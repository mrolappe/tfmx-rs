use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tfmx::TraceEvent;

#[derive(Parser)]
#[command(name = "tfmx-cli", about = "Render and inspect TFMX modules")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a song to a WAV file.
    Render(RenderArgs),
    /// Print header text, songs, tempos and layout -- static, no playback.
    Info(InfoArgs),
    /// Print the state-machine trace of a render: one line per event.
    Trace(TraceArgs),
    /// Run a song and report what the trace and the PCM say about it.
    Lint(LintArgs),
    /// Print one macro or pattern's bytecode as a linear, named listing --
    /// static, no playback. For comparing this crate's decode of an
    /// instrument/pattern against a reference by eye.
    Disasm(DisasmArgs),
    /// Compare onset timing between two rendered WAV files -- e.g. this
    /// crate's render vs. a reference player's -- via a 20ms-window
    /// RMS-derivative onset detector. Reports onset count/rate on each
    /// side and an inter-onset-interval correlation.
    OnsetDiff(OnsetDiffArgs),
}

#[derive(clap::Args)]
struct DisasmArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    /// Disassemble this macro number (0-127). Exactly one of --macro/
    /// --pattern must be given.
    #[arg(long = "macro")]
    macro_number: Option<u8>,
    /// Disassemble this pattern number (0-127). Exactly one of --macro/
    /// --pattern must be given.
    #[arg(long)]
    pattern: Option<u8>,
}

#[derive(clap::Args)]
struct RenderArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(short = 'o', long = "output")]
    output: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, default_value_t = 30)]
    seconds: u32,
    #[arg(long, default_value_t = 44_100)]
    rate: u32,
    #[arg(long, default_value_t = 100)]
    separation: u8,
    /// Mute every voice except this one (0-3).
    #[arg(long)]
    solo: Option<u8>,
    /// Mute these voices (0-3), comma-separated. Ignored if `--solo` is set.
    #[arg(long, value_delimiter = ',')]
    mute: Vec<u8>,
    /// Also render four per-voice stems (soloing each voice in turn),
    /// filenames derived from `-o` (`out.wav` -> `out-v0.wav` .. `out-v3.wav`).
    #[arg(long)]
    stems: bool,
    #[arg(long, value_enum, default_value_t = GateArg::All)]
    gate: GateArg,
}

/// Which tracks must reach `$F0 <End>` before the trackstep line advances.
/// The two readings are an open question (`docs/playback-model.md` §7); this
/// flag exists so they can be rendered and listened to side by side.
#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
enum GateArg {
    /// Every track still running a pattern must have reached `$F0`.
    All,
    /// Any one track's `$F0` moves the line, truncating the others.
    Any,
}

impl From<GateArg> for tfmx::TrackstepGate {
    fn from(arg: GateArg) -> Self {
        match arg {
            GateArg::All => tfmx::TrackstepGate::AllTracks,
            GateArg::Any => tfmx::TrackstepGate::AnyTrack,
        }
    }
}

/// `--solo`/`--mute` -> a per-voice mute mask. `--solo` wins if both are given.
/// Voice numbers outside 0-3 wrap, matching `Player`'s own `voice_of` masking.
fn resolve_mute_mask(solo: Option<u8>, mute: &[u8]) -> [bool; 4] {
    if let Some(solo) = solo {
        let solo = (solo & 0x03) as usize;
        return core::array::from_fn(|i| i != solo);
    }
    let mut mask = [false; 4];
    for &voice in mute {
        mask[(voice & 0x03) as usize] = true;
    }
    mask
}

/// `out.wav` -> `out-v0.wav` .. `out-v3.wav`, preserving directory and
/// extension (or its absence).
fn derive_stem_paths(output: &std::path::Path) -> [PathBuf; 4] {
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("out");
    let ext = output.extension().and_then(|s| s.to_str());
    core::array::from_fn(|voice| {
        let file_name = match ext {
            Some(ext) => format!("{stem}-v{voice}.{ext}"),
            None => format!("{stem}-v{voice}"),
        };
        output.with_file_name(file_name)
    })
}

#[derive(clap::Args)]
struct InfoArgs {
    mdat: PathBuf,
    smpl: PathBuf,
}

#[derive(clap::Args)]
struct LintArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, default_value_t = 30)]
    seconds: u32,
}

#[derive(clap::Args)]
struct TraceArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, default_value_t = 30)]
    seconds: u32,
    /// Restrict `Trigger`/`Voice` events to this voice (0-3).
    #[arg(long)]
    voice: Option<u8>,
    /// Restrict `Pattern` events to this track (0-7).
    #[arg(long)]
    track: Option<u8>,
    #[arg(long, value_enum, default_value_t = TraceFormat::Text)]
    format: TraceFormat,
    #[arg(long, value_enum, default_value_t = GateArg::All)]
    gate: GateArg,
}

#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
enum TraceFormat {
    Text,
}

#[derive(clap::Args)]
struct OnsetDiffArgs {
    a: PathBuf,
    b: PathBuf,
    /// Analysis window size in milliseconds.
    #[arg(long, default_value_t = 20)]
    window_ms: u32,
}

#[derive(Debug)]
enum CliError {
    Io(std::io::Error),
    Wav(hound::Error),
    Parse(tfmx::ParseError),
    Access(tfmx::AccessError),
    Usage(&'static str),
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

impl From<hound::Error> for CliError {
    fn from(e: hound::Error) -> Self {
        CliError::Wav(e)
    }
}

impl From<tfmx::ParseError> for CliError {
    fn from(e: tfmx::ParseError) -> Self {
        CliError::Parse(e)
    }
}

impl From<tfmx::AccessError> for CliError {
    fn from(e: tfmx::AccessError) -> Self {
        CliError::Access(e)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "I/O error: {e}"),
            CliError::Wav(e) => write!(f, "WAV error: {e}"),
            CliError::Parse(e) => write!(f, "invalid module: {e:?}"),
            CliError::Access(e) => write!(f, "out-of-range access: {e:?}"),
            CliError::Usage(msg) => write!(f, "usage error: {msg}"),
        }
    }
}

impl std::error::Error for CliError {}

fn render_to_wav(
    module: &tfmx::Module,
    args: &RenderArgs,
    mute: [bool; 4],
    output: &std::path::Path,
) -> Result<(), CliError> {
    let mut player = tfmx::Player::new(module, args.song, args.rate, args.separation)?;
    player.set_trackstep_gate(args.gate.into());
    for voice in 0..4u8 {
        player.set_voice_muted(voice, mute[voice as usize]);
    }

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: args.rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, spec)?;

    let total_frames = args.rate as usize * args.seconds as usize;
    let mut buf = vec![0i16; 4096 * 2];
    let mut frames_left = total_frames;
    while frames_left > 0 {
        let chunk_frames = frames_left.min(4096);
        let out = &mut buf[..chunk_frames * 2];
        player.render(out)?;
        for &sample in out.iter() {
            writer.write_sample(sample)?;
        }
        frames_left -= chunk_frames;
    }
    writer.finalize()?;
    Ok(())
}

fn run_render(args: &RenderArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    if args.stems {
        for (voice, path) in derive_stem_paths(&args.output).into_iter().enumerate() {
            let mask = resolve_mute_mask(Some(voice as u8), &[]);
            render_to_wav(&module, args, mask, &path)?;
        }
        return Ok(());
    }

    let mask = resolve_mute_mask(args.solo, &args.mute);
    render_to_wav(&module, args, mask, &args.output)
}

fn run_info(args: &InfoArgs, out: &mut impl std::io::Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    writeln!(out, "Text:")?;
    for line in module.text().chunks(40) {
        writeln!(out, "{}", String::from_utf8_lossy(line).trim_end())?;
    }

    writeln!(out, "Layout: {:?}", module.layout())?;

    writeln!(out, "Songs:")?;
    for n in 0..32u8 {
        writeln!(
            out,
            "  {n:2}: start={} end={} tempo={}",
            module.song_start(n),
            module.song_end(n),
            module.tempo(n)
        )?;
    }

    Ok(())
}

/// `docs/opcodes.md` §3's macro opcode mnemonics, `$00`-`$21`. Name only --
/// operand semantics vary per opcode (signed/unsigned, 8/16/24-bit) and are
/// already implemented once in `MacroInterpreter::execute`; re-deriving them
/// here would duplicate that match arm-for-arm. A step's raw `aa bb cc`
/// bytes are printed alongside the name instead, matching what the docs
/// table itself shows.
///
/// ponytail: name-only, not a decoded operand enum like `PatternEntry` --
/// upgrade to one (mirroring `sequencer::decode_pattern_entry`) if a
/// consumer ever needs structured macro operands, not just a printable
/// listing.
fn macro_opcode_name(op: u8) -> &'static str {
    match op {
        0x00 => "DMAoff+Reset*",
        0x01 => "DMAon",
        0x02 => "SetBegin",
        0x03 => "SetLen",
        0x04 => "Wait*",
        0x05 => "Loop",
        0x06 => "Cont",
        0x07 => "STOP*",
        0x08 => "AddNote*",
        0x09 => "SetNote*",
        0x0A => "Reset",
        0x0B => "Portamento",
        0x0C => "Vibrato",
        0x0D => "AddVolume",
        0x0E => "SetVolume",
        0x0F => "Envelope",
        0x10 => "Loop key up",
        0x11 => "AddBegin",
        0x12 => "AddLen",
        0x13 => "DMAoff*",
        0x14 => "Wait key up*",
        0x15 => "Go submacro",
        0x16 => "Return to old macro",
        0x17 => "Set period*",
        0x18 => "Sampleloop",
        0x19 => "Set one shot sample",
        0x1A => "Wait on DMA*",
        0x1B => "Random play",
        0x1C => "Splitkey",
        0x1D => "Splitvol",
        0x1E => "AddVol+Note*",
        0x1F => "SetPrevNote*",
        0x20 => "Signal",
        0x21 => "Play macro",
        _ => "?",
    }
}

/// A bounded linear listing, top to bottom, of one macro's or one pattern's
/// bytecode -- not an execution trace (see `Command::Trace` for that).
/// Stops at the opcode's own natural terminator (`$07 STOP` for a macro,
/// `$F0 End`/`$F4 STOP` for a pattern) or after `MAX_DISASM_STEPS`, whichever
/// comes first -- `pattern()`/`macro_()` return raw bytes "to the end of
/// mdat" with no length field, so an untrusted or malformed module could
/// otherwise never terminate this loop.
const MAX_DISASM_STEPS: usize = 256;

fn run_disasm(args: &DisasmArgs, out: &mut impl Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    match (args.macro_number, args.pattern) {
        (Some(n), None) => {
            let bytes = module.macro_(n)?;
            for (step, word) in bytes.chunks_exact(4).take(MAX_DISASM_STEPS).enumerate() {
                let [op, aa, bb, cc] = [word[0], word[1], word[2], word[3]];
                writeln!(
                    out,
                    "{step:4}: ${op:02X} <{}> aa=${aa:02X} bb=${bb:02X} cc=${cc:02X}",
                    macro_opcode_name(op)
                )?;
                if op == 0x07 {
                    break;
                }
            }
        }
        (None, Some(n)) => {
            let bytes = module.pattern(n)?;
            for (step, word) in bytes.chunks_exact(4).take(MAX_DISASM_STEPS).enumerate() {
                let entry = tfmx::decode_pattern_entry([word[0], word[1], word[2], word[3]]);
                writeln!(out, "{step:4}: {entry:?}")?;
                if matches!(
                    entry,
                    tfmx::PatternEntry::Command(
                        tfmx::PatternCommand::End | tfmx::PatternCommand::Stop
                    )
                ) {
                    break;
                }
            }
        }
        _ => return Err(CliError::Usage("pass exactly one of --macro or --pattern")),
    }
    Ok(())
}

/// One `TraceEvent` as one aligned, greppable text line. The one-function-
/// per-format seam `docs/status.md`'s M4 plan calls for: JSON/TOON later is
/// a new function plus one `TraceFormat` arm, not a trait.
fn write_text_event(e: &TraceEvent, out: &mut impl Write) -> std::io::Result<()> {
    match e {
        TraceEvent::Jiffy {
            frame,
            line,
            tempo,
            stopped,
        } => writeln!(
            out,
            "JIFFY     frame={frame} line={line} tempo={tempo} stopped={stopped}"
        ),
        TraceEvent::Trackstep(line) => writeln!(out, "TRACKSTEP {line:?}"),
        TraceEvent::Pattern {
            track,
            pattern,
            step,
            entry,
        } => writeln!(
            out,
            "PATTERN   track={track} pattern={pattern} step={step} entry={entry:?}"
        ),
        TraceEvent::Trigger {
            voice,
            macro_number,
            note,
            volume,
            transpose,
        } => writeln!(
            out,
            "TRIGGER   voice={voice} macro={macro_number} note={note} volume={volume} transpose={transpose}"
        ),
        TraceEvent::Voice { voice, state } => {
            writeln!(out, "VOICE     voice={voice} state={state:?}")
        }
    }
}

/// Folds a trace event stream to text: `Jiffy`/`Trackstep` events always
/// pass through as timeline context; `Pattern` is restricted to `track` and
/// `Trigger`/`Voice` to `voice` when given; a `Voice` event is dropped when
/// its state is unchanged from the last one emitted for that voice -- the
/// noise a diagnostic actually needs cut (step 11.4).
fn write_trace(
    events: &[TraceEvent],
    voice: Option<u8>,
    track: Option<u8>,
    out: &mut impl Write,
) -> std::io::Result<()> {
    let mut last_voice_state: [Option<tfmx::Voice>; 4] = [None; 4];
    for e in events {
        match e {
            TraceEvent::Pattern { track: t, .. } if track.is_some_and(|f| f != *t) => continue,
            TraceEvent::Trigger { voice: v, .. } if voice.is_some_and(|f| f != *v) => continue,
            TraceEvent::Voice { voice: v, state } => {
                if voice.is_some_and(|f| f != *v) {
                    continue;
                }
                let slot = &mut last_voice_state[*v as usize];
                if *slot == Some(*state) {
                    continue;
                }
                *slot = Some(*state);
            }
            _ => {}
        }
        write_text_event(e, out)?;
    }
    Ok(())
}

fn run_trace(args: &TraceArgs, out: &mut impl Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    const SAMPLE_RATE: u32 = 44_100;
    const SEPARATION: u8 = 100;
    let mut player = tfmx::Player::new(&module, args.song, SAMPLE_RATE, SEPARATION)?;
    player.set_trackstep_gate(args.gate.into());

    let mut events = Vec::new();
    let total_frames = SAMPLE_RATE as usize * args.seconds as usize;
    let mut buf = vec![0i16; 4096 * 2];
    let mut frames_left = total_frames;
    while frames_left > 0 {
        let chunk_frames = frames_left.min(4096);
        player.render_traced(&mut buf[..chunk_frames * 2], |e| events.push(e))?;
        frames_left -= chunk_frames;
    }

    match args.format {
        TraceFormat::Text => write_trace(&events, args.voice, args.track, out)?,
    }
    Ok(())
}

/// One thing worth looking at, named so a test (and a grep over a corpus
/// run) can key on it. `detail` says which voice/opcode/second it is about.
#[derive(Debug)]
struct Finding {
    name: &'static str,
    detail: String,
}

/// Everything `lint` has to say about one render: the summary the roadmap
/// lists, plus the findings derived from it.
#[derive(Debug, Default)]
struct Report {
    jiffies: u64,
    frames: u64,
    tempos: BTreeSet<u16>,
    /// Distinct trackstep line indices visited.
    lines: BTreeSet<u16>,
    /// A line index was revisited after some other line ran in between.
    looped: bool,
    /// Frame of the first jiffy that saw the player already halted.
    stopped_at: Option<u64>,
    /// Distinct pattern numbers executed, per track.
    patterns: [BTreeSet<u8>; 8],
    /// Distinct macro numbers triggered, per voice.
    macros: [BTreeSet<u8>; 4],
    note_ons: [u32; 4],
    /// Pattern commands by variant name (`Wait`, `Loop`, ...).
    commands: BTreeMap<String, u32>,
    unsupported: Vec<(u8, u32)>,
    peak: i32,
    clipped: usize,
    samples: usize,
    findings: Vec<Finding>,
}

/// Same period, volume and sample region with DMA on for longer than this
/// means the voice is stuck, not sustaining.
const FROZEN_SECONDS: f64 = 2.0;
/// Peak amplitude below this (of 32767) is silence for reporting purposes.
const SILENCE_PEAK: i32 = 32;
/// Fraction of full-scale samples above which the mix is judged to clip.
const CLIP_FRACTION: f64 = 0.001;

/// The whole of `lint`'s analysis, as a pure function: `events` is the trace
/// of a render, `unsupported` its `(opcode, count)` pairs and `pcm` the
/// interleaved stereo samples it produced. Nothing here touches a `Player`
/// or the filesystem, so every finding is drivable from a hand-built vector.
///
/// The unsupported-opcode counts and the PCM cannot come out of the event
/// stream (they live on `Player::unsupported_ops` and in the output buffer),
/// so they are passed in alongside it rather than being smuggled into
/// `TraceEvent` -- keeping the trace seam what `docs/architecture.md` §2
/// says it is, a record of state-machine transitions only.
fn lint(events: &[TraceEvent], unsupported: &[(u8, u32)], pcm: &[i16], rate: u32) -> Report {
    let mut r = Report {
        unsupported: unsupported.iter().copied().filter(|&(_, n)| n > 0).collect(),
        ..Default::default()
    };

    // Per-voice state carried across the fold.
    let mut alive = [false; 4];
    /// (period, volume, start, len) -- what "unchanged" means for a voice.
    type VoiceKey = (u16, u8, u32, u32);
    let mut held: [Option<(VoiceKey, u64)>; 4] = [None; 4];
    let mut frozen_max = [0u64; 4];
    let mut regions: [BTreeSet<(u32, u32)>; 4] = Default::default();

    let mut frame = 0u64;
    let mut last_line: Option<u16> = None;

    for e in events {
        match e {
            TraceEvent::Jiffy {
                frame: f,
                line,
                tempo,
                stopped,
            } => {
                frame = *f;
                r.frames = r.frames.max(*f);
                r.jiffies += 1;
                r.tempos.insert(*tempo);
                if last_line != Some(*line) {
                    if r.lines.contains(line) {
                        r.looped = true;
                    }
                    last_line = Some(*line);
                }
                r.lines.insert(*line);
                if *stopped && r.stopped_at.is_none() {
                    r.stopped_at = Some(*f);
                }
            }
            TraceEvent::Trackstep(_) => {}
            TraceEvent::Pattern {
                track,
                pattern,
                entry,
                ..
            } => {
                r.patterns[(*track as usize) & 7].insert(*pattern);
                if let tfmx::PatternEntry::Command(c) = entry {
                    // Variant name off `Debug` -- one histogram key per
                    // command without a 16-arm match to keep in sync.
                    let name = format!("{c:?}");
                    let name = name.split([' ', '(']).next().unwrap_or(&name).to_string();
                    *r.commands.entry(name).or_default() += 1;
                }
            }
            TraceEvent::Trigger {
                voice,
                macro_number,
                ..
            } => {
                let v = (*voice as usize) & 3;
                r.macros[v].insert(*macro_number);
                r.note_ons[v] += 1;
            }
            TraceEvent::Voice { voice, state } => {
                let v = (*voice as usize) & 3;
                if !state.dma_on {
                    held[v] = None;
                    continue;
                }
                alive[v] = true;
                regions[v].insert((state.start, state.len));
                let key = (state.period, state.volume, state.start, state.len);
                match held[v] {
                    Some((k, since)) if k == key => {
                        frozen_max[v] = frozen_max[v].max(frame.saturating_sub(since));
                    }
                    _ => held[v] = Some((key, frame)),
                }
            }
        }
    }

    r.samples = pcm.len();
    r.peak = pcm.iter().map(|&s| (s as i32).abs()).max().unwrap_or(0);
    r.clipped = pcm
        .iter()
        .filter(|&&s| s == i16::MIN || s == i16::MAX)
        .count();

    let seconds = |frames: u64| frames as f64 / rate.max(1) as f64;
    let frozen_frames = (FROZEN_SECONDS * rate as f64) as u64;

    let dead: Vec<String> = (0..4)
        .filter(|&v| !alive[v])
        .map(|v| v.to_string())
        .collect();
    if !dead.is_empty() {
        r.findings.push(Finding {
            name: "dead-voice",
            detail: format!("DMA never on for voice(s) {}", dead.join(", ")),
        });
    }

    for v in 0..4 {
        if frozen_max[v] > frozen_frames {
            r.findings.push(Finding {
                name: "frozen-voice",
                detail: format!(
                    "voice {v}: period/volume/region unchanged for {:.1} s with DMA on",
                    seconds(frozen_max[v])
                ),
            });
        }
        if alive[v] && regions[v].len() <= 1 {
            // The region itself matters: `start=0 len=0` is a voice that is
            // on but playing nothing, a real fragment is one stuck sample.
            let (start, len) = regions[v].iter().next().copied().unwrap_or((0, 0));
            r.findings.push(Finding {
                name: "no-retrigger",
                detail: format!(
                    "voice {v}: one sample region for the whole run (start={start} len={len})"
                ),
            });
        }
    }

    let distinct_patterns: BTreeSet<u8> = r.patterns.iter().flatten().copied().collect();
    if distinct_patterns.len() == 1 {
        r.findings.push(Finding {
            name: "single-pattern",
            detail: format!("only pattern {:?} ever ran", distinct_patterns),
        });
    }

    if let Some(at) = r.stopped_at {
        r.findings.push(Finding {
            name: "stopped-early",
            detail: format!(
                "player halted at {:.1} s of {:.1} s",
                seconds(at),
                seconds(r.frames)
            ),
        });
    }

    if !pcm.is_empty() && r.peak < SILENCE_PEAK {
        r.findings.push(Finding {
            name: "silence",
            detail: format!("peak amplitude {} of 32767", r.peak),
        });
    }

    if !pcm.is_empty() && r.clipped as f64 > CLIP_FRACTION * pcm.len() as f64 {
        r.findings.push(Finding {
            name: "clipping",
            detail: format!(
                "{} of {} samples at full scale ({:.2}%)",
                r.clipped,
                pcm.len(),
                100.0 * r.clipped as f64 / pcm.len() as f64
            ),
        });
    }

    if !r.unsupported.is_empty() {
        let list: Vec<String> = r
            .unsupported
            .iter()
            .map(|(op, n)| format!("${op:02X}x{n}"))
            .collect();
        r.findings.push(Finding {
            name: "unsupported-ops",
            detail: list.join(" "),
        });
    }

    r
}

fn write_report(r: &Report, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(out, "Jiffies: {}", r.jiffies)?;
    writeln!(out, "Tempos: {:?}", r.tempos)?;
    writeln!(
        out,
        "Trackstep lines: {} distinct, looped={}, stopped={}",
        r.lines.len(),
        r.looped,
        r.stopped_at.is_some()
    )?;
    for (track, patterns) in r.patterns.iter().enumerate() {
        if !patterns.is_empty() {
            writeln!(out, "Track {track}: {} patterns {patterns:?}", patterns.len())?;
        }
    }
    for voice in 0..4 {
        writeln!(
            out,
            "Voice {voice}: {} note-ons, macros {:?}",
            r.note_ons[voice], r.macros[voice]
        )?;
    }
    writeln!(out, "Pattern commands: {:?}", r.commands)?;
    if r.unsupported.is_empty() {
        writeln!(out, "Unsupported ops: (none)")?;
    } else {
        writeln!(out, "Unsupported ops:")?;
        for (op, n) in &r.unsupported {
            writeln!(out, "  ${op:02X}: {n}")?;
        }
    }
    writeln!(
        out,
        "PCM: {} samples, peak {}, {} clipped",
        r.samples, r.peak, r.clipped
    )?;
    if r.findings.is_empty() {
        writeln!(out, "Findings: (none)")?;
    } else {
        writeln!(out, "Findings:")?;
        for f in &r.findings {
            writeln!(out, "  {}: {}", f.name, f.detail)?;
        }
    }
    Ok(())
}

fn run_lint(args: &LintArgs, out: &mut impl Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    const SAMPLE_RATE: u32 = 44_100;
    const SEPARATION: u8 = 100;
    let mut player = tfmx::Player::new(&module, args.song, SAMPLE_RATE, SEPARATION)?;

    let total_frames = SAMPLE_RATE as usize * args.seconds as usize;
    let mut events = Vec::new();
    let mut pcm = vec![0i16; total_frames * 2];
    for chunk in pcm.chunks_mut(4096 * 2) {
        player.render_traced(chunk, |e| events.push(e))?;
    }
    let unsupported: Vec<(u8, u32)> = (0..=255u8)
        .map(|op| (op, player.unsupported_ops().get(op)))
        .filter(|&(_, n)| n > 0)
        .collect();

    let report = lint(&events, &unsupported, &pcm, SAMPLE_RATE);
    write_report(&report, out)?;
    Ok(())
}

/// A window's RMS must clear this floor to ever count as an onset -- keeps
/// quiet hiss/dither from registering.
const ONSET_NOISE_FLOOR: f64 = 128.0;
/// An onset is a window whose RMS exceeds the previous window's by this
/// ratio (the "threshold-jump" from the ad hoc method this promotes).
const ONSET_JUMP_RATIO: f64 = 1.5;

/// Reads a WAV file and downmixes to mono `i16`, returning `(samples, rate)`.
fn read_wav_mono(path: &std::path::Path) -> Result<(Vec<i16>, u32), CliError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<_, _>>()?;
    let mono = if spec.channels <= 1 {
        samples
    } else {
        let channels = spec.channels as usize;
        samples
            .chunks(channels)
            .map(|frame| (frame.iter().map(|&s| s as i32).sum::<i32>() / channels as i32) as i16)
            .collect()
    };
    Ok((mono, spec.sample_rate))
}

/// RMS amplitude of each non-overlapping `window_ms` window.
fn window_rms(mono: &[i16], rate: u32, window_ms: u32) -> Vec<f64> {
    let window_len = ((rate as u64 * window_ms as u64 / 1000).max(1)) as usize;
    mono.chunks(window_len)
        .map(|w| {
            let sum_sq: f64 = w.iter().map(|&s| (s as f64) * (s as f64)).sum();
            (sum_sq / w.len() as f64).sqrt()
        })
        .collect()
}

/// Onset timestamps (seconds), via a 20ms-window RMS-derivative threshold
/// jump: a rising edge (a jumped window preceded by a non-jumped one) counts
/// once, however many further windows the same attack ramp keeps jumping
/// for. ponytail: fixed ratio/floor, whole-mix RMS -- promotes the ad hoc
/// method already validated by hand across several sessions (see
/// `docs/status.md`), which only ever applied it to silence-anchored
/// onsets (a piece's first note, an all-voice stop). **Known ceiling**: in
/// continuous polyphonic material the full mix rarely dips back near the
/// noise floor between notes, so a new note arriving over still-ringing
/// voices is invisible to this detector -- corpus-wide it undercounts
/// density far more on dense material than on sparse. Per-voice (not
/// full-mix) comparison would raise the ceiling, but the only reference
/// player available (`uade123`) has no per-voice solo/mute output to diff
/// against.
fn detect_onsets(mono: &[i16], rate: u32, window_ms: u32) -> Vec<f64> {
    let rms = window_rms(mono, rate, window_ms);
    let mut onsets = Vec::new();
    let mut in_onset = false;
    for i in 1..rms.len() {
        let jumped = rms[i] > ONSET_NOISE_FLOOR && rms[i] > rms[i - 1] * ONSET_JUMP_RATIO;
        if jumped && !in_onset {
            onsets.push(i as f64 * window_ms as f64 / 1000.0);
        }
        in_onset = jumped;
    }
    onsets
}

fn inter_onset_intervals(onsets: &[f64]) -> Vec<f64> {
    onsets.windows(2).map(|w| w[1] - w[0]).collect()
}

/// Pearson correlation over the shorter of the two slices' lengths, paired
/// by index. `None` if fewer than two points, or either side is constant.
fn pearson_correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let (a, b) = (&a[..n], &b[..n]);
    let mean_a = a.iter().sum::<f64>() / n as f64;
    let mean_b = b.iter().sum::<f64>() / n as f64;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..n {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    if var_a == 0.0 || var_b == 0.0 {
        return None;
    }
    Some(cov / (var_a.sqrt() * var_b.sqrt()))
}

fn run_onset_diff(args: &OnsetDiffArgs, out: &mut impl Write) -> Result<(), CliError> {
    let (mono_a, rate_a) = read_wav_mono(&args.a)?;
    let (mono_b, rate_b) = read_wav_mono(&args.b)?;
    let onsets_a = detect_onsets(&mono_a, rate_a, args.window_ms);
    let onsets_b = detect_onsets(&mono_b, rate_b, args.window_ms);
    let duration_a = mono_a.len() as f64 / rate_a as f64;
    let duration_b = mono_b.len() as f64 / rate_b as f64;
    let rate_per_sec = |onsets: &[f64], duration: f64| {
        if duration > 0.0 {
            onsets.len() as f64 / duration
        } else {
            0.0
        }
    };

    writeln!(
        out,
        "a: {} onsets over {:.1}s ({:.1}/s)",
        onsets_a.len(),
        duration_a,
        rate_per_sec(&onsets_a, duration_a)
    )?;
    writeln!(
        out,
        "b: {} onsets over {:.1}s ({:.1}/s)",
        onsets_b.len(),
        duration_b,
        rate_per_sec(&onsets_b, duration_b)
    )?;

    let ioi_a = inter_onset_intervals(&onsets_a);
    let ioi_b = inter_onset_intervals(&onsets_b);
    match pearson_correlation(&ioi_a, &ioi_b) {
        Some(r) => writeln!(out, "inter-onset-interval correlation: {r:.3}")?,
        None => writeln!(
            out,
            "inter-onset-interval correlation: n/a (fewer than 2 intervals on one side)"
        )?,
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Render(args) => run_render(args),
        Command::Info(args) => run_info(args, &mut std::io::stdout().lock()),
        Command::Trace(args) => run_trace(args, &mut std::io::stdout().lock()),
        Command::Lint(args) => run_lint(args, &mut std::io::stdout().lock()),
        Command::Disasm(args) => run_disasm(args, &mut std::io::stdout().lock()),
        Command::OnsetDiff(args) => run_onset_diff(args, &mut std::io::stdout().lock()),
    };
    if let Err(e) = result {
        eprintln!("tfmx-cli: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(path).ok()
    }

    fn corpus_path(name: &str) -> Option<PathBuf> {
        let path = PathBuf::from(format!(
            "{}/../testdata/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        ));
        path.exists().then_some(path)
    }

    /// Step 5.2's own check: `info` reports the header text, the song/tempo
    /// table and the detected layout -- cross-checked against the known
    /// values already established by `tfmx::Module`'s own tests.
    #[test]
    fn info_prints_text_layout_and_songs() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");

        let args = InfoArgs { mdat, smpl };
        let mut out = Vec::new();
        run_info(&args, &mut out).expect("info succeeds on a valid corpus file");
        let text = String::from_utf8(out).expect("output is UTF-8");

        assert!(text.contains("(Empty)"));
        assert!(text.contains("Layout: Fixed"));
        assert!(text.contains("0: start=75 end=129 tempo=3"));
        assert!(text.contains("1: start=52 end=74 tempo=120"));
        assert!(
            !text.contains("Unsupported ops:"),
            "info is static since step 11.5 -- playback findings live in `lint`"
        );
    }

    /// Step 5.2's roadmap check: `info` runs across the whole corpus.
    #[test]
    fn info_runs_across_full_corpus_without_error() {
        let names = [
            "turrican intro",
            "turrican outside",
            "r-type",
            "x-out (title)",
            "turrican 2 title (st)",
            "turrican 2 level 1-desert",
            "turrican 2 level 3-flight",
            "turrican 3 level 1",
            "apidya (title)",
            "apidya (level 1)",
        ];

        for name in names {
            let Some(mdat) = corpus_path(&format!("mdat.{name}")) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            };
            let smpl = corpus_path(&format!("smpl.{name}")).expect("smpl present alongside mdat");

            let args = InfoArgs { mdat, smpl };
            let mut out = Vec::new();
            run_info(&args, &mut out).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    /// Step 5.1's own check: `render` produces a playable WAV of the
    /// requested length.
    #[test]
    fn render_writes_a_wav_of_the_requested_length() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let mdat_path = std::env::temp_dir().join("tfmx-cli-test-input.mdat");
        let smpl_path = std::env::temp_dir().join("tfmx-cli-test-input.smpl");
        std::fs::write(&mdat_path, &mdat).unwrap();
        std::fs::write(&smpl_path, &smpl).unwrap();
        let output = std::env::temp_dir().join("tfmx-cli-test-output.wav");

        let args = RenderArgs {
            mdat: mdat_path,
            smpl: smpl_path,
            output: output.clone(),
            song: 0,
            seconds: 1,
            rate: 44_100,
            separation: 100,
            solo: None,
            mute: Vec::new(),
            stems: false,
            gate: GateArg::All,
        };
        run_render(&args).expect("render succeeds on a valid corpus file");

        let reader = hound::WavReader::open(&output).expect("output is a valid WAV file");
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 44_100);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(reader.duration(), 44_100, "WAV must hold exactly 1 second");

        std::fs::remove_file(&output).ok();
    }

    #[test]
    fn resolve_mute_mask_with_neither_flag_mutes_nothing() {
        assert_eq!(resolve_mute_mask(None, &[]), [false; 4]);
    }

    #[test]
    fn resolve_mute_mask_mutes_the_listed_voices() {
        assert_eq!(
            resolve_mute_mask(None, &[1, 3]),
            [false, true, false, true]
        );
    }

    #[test]
    fn resolve_mute_mask_solo_mutes_every_other_voice() {
        assert_eq!(resolve_mute_mask(Some(2), &[]), [true, true, false, true]);
    }

    #[test]
    fn resolve_mute_mask_solo_overrides_mute_list() {
        assert_eq!(
            resolve_mute_mask(Some(0), &[1, 2, 3]),
            [false, true, true, true]
        );
    }

    #[test]
    fn derive_stem_paths_appends_voice_suffix_before_extension() {
        let paths = derive_stem_paths(std::path::Path::new("out.wav"));
        assert_eq!(
            paths,
            [
                PathBuf::from("out-v0.wav"),
                PathBuf::from("out-v1.wav"),
                PathBuf::from("out-v2.wav"),
                PathBuf::from("out-v3.wav"),
            ]
        );
    }

    #[test]
    fn derive_stem_paths_preserves_directory() {
        let paths = derive_stem_paths(std::path::Path::new("/tmp/foo/out.wav"));
        assert_eq!(paths[0], PathBuf::from("/tmp/foo/out-v0.wav"));
        assert_eq!(paths[3], PathBuf::from("/tmp/foo/out-v3.wav"));
    }

    #[test]
    fn derive_stem_paths_without_extension_has_no_dot() {
        let paths = derive_stem_paths(std::path::Path::new("out"));
        assert_eq!(paths[0], PathBuf::from("out-v0"));
    }

    fn voice_state(period: u16, volume: u8) -> tfmx::Voice {
        let mut state = tfmx::Voice::default();
        state.start = 100;
        state.len = 50;
        state.period = period;
        state.volume = volume;
        state.dma_on = true;
        state
    }

    #[test]
    fn write_trace_formats_a_jiffy_event() {
        let events = vec![TraceEvent::Jiffy {
            frame: 42,
            line: 5,
            tempo: 6,
            stopped: false,
        }];
        let mut out = Vec::new();
        write_trace(&events, None, None, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "JIFFY     frame=42 line=5 tempo=6 stopped=false\n"
        );
    }

    #[test]
    fn write_trace_formats_a_trackstep_event() {
        let events = vec![TraceEvent::Trackstep(tfmx::TrackstepLine::Command(
            tfmx::LineCommand::Stop,
        ))];
        let mut out = Vec::new();
        write_trace(&events, None, None, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "TRACKSTEP Command(Stop)\n"
        );
    }

    #[test]
    fn write_trace_keeps_only_the_requested_track_pattern_events() {
        let events = vec![
            TraceEvent::Pattern {
                track: 0,
                pattern: 1,
                step: 0,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::End),
            },
            TraceEvent::Pattern {
                track: 1,
                pattern: 2,
                step: 0,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::End),
            },
        ];
        let mut out = Vec::new();
        write_trace(&events, None, Some(1), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("track=0"));
        assert!(text.contains("track=1"));
    }

    #[test]
    fn write_trace_keeps_only_the_requested_voice_trigger_events() {
        let events = vec![
            TraceEvent::Trigger {
                voice: 0,
                macro_number: 1,
                note: 12,
                volume: 64,
                transpose: 0,
            },
            TraceEvent::Trigger {
                voice: 2,
                macro_number: 3,
                note: 24,
                volume: 64,
                transpose: 0,
            },
        ];
        let mut out = Vec::new();
        write_trace(&events, Some(2), None, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("voice=0"));
        assert!(text.contains("voice=2"));
    }

    #[test]
    fn write_trace_suppresses_an_unchanged_voice_event() {
        let state = voice_state(428, 64);
        let events = vec![
            TraceEvent::Voice { voice: 0, state },
            TraceEvent::Voice { voice: 0, state },
        ];
        let mut out = Vec::new();
        write_trace(&events, None, None, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("VOICE").count(), 1);
    }

    #[test]
    fn write_trace_emits_a_changed_voice_event_again() {
        let events = vec![
            TraceEvent::Voice {
                voice: 0,
                state: voice_state(428, 64),
            },
            TraceEvent::Voice {
                voice: 0,
                state: voice_state(214, 64),
            },
        ];
        let mut out = Vec::new();
        write_trace(&events, None, None, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("VOICE").count(), 2);
    }

    #[test]
    fn write_trace_tracks_unchanged_state_independently_per_voice() {
        let state = voice_state(428, 64);
        let events = vec![
            TraceEvent::Voice { voice: 0, state },
            TraceEvent::Voice { voice: 1, state },
        ];
        let mut out = Vec::new();
        write_trace(&events, None, None, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.matches("VOICE").count(), 2);
    }

    const RATE: u32 = 44_100;

    fn jiffy(frame: u64, line: u16, stopped: bool) -> TraceEvent {
        TraceEvent::Jiffy {
            frame,
            line,
            tempo: 6,
            stopped,
        }
    }

    /// A voice state whose sample region is picked by `start`, so a test can
    /// say "same region" or "new region" without touching the other fields.
    fn region_state(start: u32, period: u16, dma_on: bool) -> tfmx::Voice {
        let mut state = tfmx::Voice::default();
        state.start = start;
        state.len = 50;
        state.period = period;
        state.volume = 64;
        state.dma_on = dma_on;
        state
    }

    fn finding<'a>(report: &'a Report, name: &str) -> Option<&'a Finding> {
        report.findings.iter().find(|f| f.name == name)
    }

    /// Enough loud, varying audio that the PCM findings stay quiet -- so a
    /// test of an event-stream finding only ever fires that one finding.
    fn healthy_pcm() -> Vec<i16> {
        (0..2000).map(|i| if i % 2 == 0 { 8000 } else { -8000 }).collect()
    }

    #[test]
    fn lint_flags_a_voice_whose_dma_never_turns_on() {
        let events = vec![
            jiffy(0, 0, false),
            TraceEvent::Voice {
                voice: 0,
                state: region_state(100, 428, true),
            },
            TraceEvent::Voice {
                voice: 3,
                state: region_state(0, 0, false),
            },
        ];
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        let f = finding(&report, "dead-voice").expect("dead-voice fires for a voice with no DMA");
        assert!(f.detail.contains('3'), "names the dead voice: {}", f.detail);
        assert!(!f.detail.contains('0'), "voice 0 is alive: {}", f.detail);
    }

    #[test]
    fn lint_flags_a_voice_frozen_for_more_than_two_seconds() {
        let state = region_state(100, 428, true);
        let mut events = Vec::new();
        for i in 0..=3u64 {
            events.push(jiffy(i * RATE as u64, 0, false));
            events.push(TraceEvent::Voice { voice: 0, state });
        }
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        assert!(finding(&report, "frozen-voice").is_some());
    }

    #[test]
    fn lint_does_not_flag_a_voice_that_keeps_changing() {
        let mut events = Vec::new();
        for i in 0..=3u64 {
            events.push(jiffy(i * RATE as u64, 0, false));
            events.push(TraceEvent::Voice {
                voice: 0,
                state: region_state(100 + i as u32 * 10, 428 + i as u16, true),
            });
        }
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        assert!(finding(&report, "frozen-voice").is_none());
        assert!(finding(&report, "no-retrigger").is_none());
    }

    /// The `apidya (title)` symptom: the voice keeps moving, but it is always
    /// the same sample region -- one fragment looping for the whole run.
    #[test]
    fn lint_flags_a_voice_whose_sample_region_never_changes() {
        let mut events = Vec::new();
        for i in 0..=3u64 {
            events.push(jiffy(i * RATE as u64, 0, false));
            events.push(TraceEvent::Voice {
                voice: 0,
                state: region_state(100, 428 + i as u16, true),
            });
        }
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        let f = finding(&report, "no-retrigger").expect("no-retrigger fires");
        assert!(
            f.detail.contains("start=100"),
            "names the stuck region: {}",
            f.detail
        );
        assert!(
            finding(&report, "frozen-voice").is_none(),
            "a moving period is not frozen"
        );
    }

    #[test]
    fn lint_flags_a_run_that_only_ever_visits_one_pattern() {
        let events = vec![
            jiffy(0, 0, false),
            TraceEvent::Pattern {
                track: 0,
                pattern: 7,
                step: 0,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::Nop),
            },
            TraceEvent::Pattern {
                track: 1,
                pattern: 7,
                step: 1,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::Nop),
            },
        ];
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        assert!(finding(&report, "single-pattern").is_some());
    }

    #[test]
    fn lint_does_not_flag_single_pattern_when_two_patterns_run() {
        let events = vec![
            jiffy(0, 0, false),
            TraceEvent::Pattern {
                track: 0,
                pattern: 7,
                step: 0,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::Nop),
            },
            TraceEvent::Pattern {
                track: 0,
                pattern: 8,
                step: 0,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::Nop),
            },
        ];
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        assert!(finding(&report, "single-pattern").is_none());
    }

    #[test]
    fn lint_flags_a_run_that_stops_before_the_end() {
        let events = vec![
            jiffy(0, 0, false),
            jiffy(RATE as u64, 1, true),
            jiffy(2 * RATE as u64, 1, true),
        ];
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        let f = finding(&report, "stopped-early").expect("stopped-early fires");
        assert!(f.detail.contains("1.0"), "reports when: {}", f.detail);
    }

    #[test]
    fn lint_flags_a_silent_render() {
        let events = vec![jiffy(0, 0, false)];
        let report = lint(&events, &[], &vec![0i16; 2000], RATE);
        assert!(finding(&report, "silence").is_some());
    }

    #[test]
    fn lint_flags_a_clipping_render() {
        let mut pcm = vec![1000i16; 2000];
        pcm[..100].fill(i16::MAX);
        let report = lint(&[jiffy(0, 0, false)], &[], &pcm, RATE);
        let f = finding(&report, "clipping").expect("clipping fires");
        assert!(f.detail.contains("100"), "reports the count: {}", f.detail);
    }

    #[test]
    fn lint_does_not_flag_clipping_on_a_healthy_render() {
        let report = lint(&[jiffy(0, 0, false)], &[], &healthy_pcm(), RATE);
        assert!(finding(&report, "clipping").is_none());
        assert!(finding(&report, "silence").is_none());
    }

    #[test]
    fn lint_flags_unsupported_opcodes() {
        let report = lint(&[jiffy(0, 0, false)], &[(0x22, 7)], &healthy_pcm(), RATE);
        let f = finding(&report, "unsupported-ops").expect("unsupported-ops fires");
        assert!(f.detail.contains("$22"), "names the opcode: {}", f.detail);
    }

    #[test]
    fn lint_summarizes_jiffies_tempos_and_trackstep_lines() {
        let events = vec![
            TraceEvent::Jiffy {
                frame: 0,
                line: 0,
                tempo: 6,
                stopped: false,
            },
            TraceEvent::Jiffy {
                frame: 100,
                line: 1,
                tempo: 6,
                stopped: false,
            },
            TraceEvent::Jiffy {
                frame: 200,
                line: 0,
                tempo: 3,
                stopped: false,
            },
        ];
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        assert_eq!(report.jiffies, 3);
        assert_eq!(report.tempos.iter().copied().collect::<Vec<_>>(), [3, 6]);
        assert_eq!(report.lines.len(), 2);
        assert!(report.looped, "line 0 is revisited after line 1");
        assert_eq!(report.stopped_at, None);
    }

    #[test]
    fn lint_counts_patterns_macros_note_ons_and_commands() {
        let events = vec![
            jiffy(0, 0, false),
            TraceEvent::Pattern {
                track: 2,
                pattern: 7,
                step: 0,
                entry: tfmx::PatternEntry::Note {
                    note: 24,
                    macro_number: 3,
                    volume: 64,
                    voice: 1,
                    timing: tfmx::NoteTiming::Detune(0),
                },
            },
            TraceEvent::Pattern {
                track: 2,
                pattern: 8,
                step: 0,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::Wait { jiffies: 2 }),
            },
            TraceEvent::Pattern {
                track: 2,
                pattern: 8,
                step: 1,
                entry: tfmx::PatternEntry::Command(tfmx::PatternCommand::Wait { jiffies: 3 }),
            },
            TraceEvent::Trigger {
                voice: 1,
                macro_number: 3,
                note: 24,
                volume: 64,
                transpose: 0,
            },
            TraceEvent::Trigger {
                voice: 1,
                macro_number: 5,
                note: 26,
                volume: 64,
                transpose: 0,
            },
        ];
        let report = lint(&events, &[], &healthy_pcm(), RATE);
        assert_eq!(report.patterns[2].len(), 2);
        assert_eq!(report.patterns[0].len(), 0);
        assert_eq!(report.macros[1].len(), 2);
        assert_eq!(report.note_ons[1], 2);
        assert_eq!(report.commands.get("Wait"), Some(&2));
    }

    #[test]
    fn write_report_prints_the_summary_and_every_finding() {
        let events = vec![jiffy(0, 0, false)];
        let report = lint(&events, &[(0x22, 1)], &vec![0i16; 2000], RATE);
        let mut out = Vec::new();
        write_report(&report, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Jiffies: 1"));
        assert!(text.contains("silence"));
        assert!(text.contains("unsupported-ops"));
    }

    /// Step 11.5's roadmap check: `lint` runs across the whole corpus.
    #[test]
    fn lint_runs_across_full_corpus_without_error() {
        let names = [
            "turrican intro",
            "turrican outside",
            "r-type",
            "x-out (title)",
            "turrican 2 title (st)",
            "turrican 2 level 1-desert",
            "turrican 2 level 3-flight",
            "turrican 3 level 1",
            "apidya (title)",
            "apidya (level 1)",
        ];

        for name in names {
            let Some(mdat) = corpus_path(&format!("mdat.{name}")) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            };
            let smpl = corpus_path(&format!("smpl.{name}")).expect("smpl present alongside mdat");

            let args = LintArgs {
                mdat,
                smpl,
                song: 0,
                seconds: 2,
            };
            let mut out = Vec::new();
            run_lint(&args, &mut out).unwrap_or_else(|e| panic!("{name}: {e}"));
            let text = String::from_utf8(out).expect("output is UTF-8");
            assert!(text.contains("Jiffies:"), "{name}: report has a summary");
        }
    }

    /// Step 11.4's roadmap check: a corpus run of two songs produces
    /// visibly different traces.
    #[test]
    fn trace_of_two_different_songs_differs() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");

        let mut song0 = Vec::new();
        run_trace(
            &TraceArgs {
                mdat: mdat.clone(),
                smpl: smpl.clone(),
                song: 0,
                seconds: 1,
                voice: None,
                track: None,
                format: TraceFormat::Text,
                gate: GateArg::All,
            },
            &mut song0,
        )
        .expect("trace succeeds on a valid corpus file");

        let mut song1 = Vec::new();
        run_trace(
            &TraceArgs {
                mdat,
                smpl,
                song: 1,
                seconds: 1,
                voice: None,
                track: None,
                format: TraceFormat::Text,
                gate: GateArg::All,
            },
            &mut song1,
        )
        .expect("trace succeeds on a valid corpus file");

        assert_ne!(song0, song1);
    }

    /// `turrican intro`'s macro 24 -- the keysplit/`Cont` instrument at the
    /// centre of this session's retrigger fix (`MacroInterpreter::instrument`).
    /// Fixes this exact bytecode as a regression check: a Splitkey into two
    /// `Cont`s, terminated by `STOP`.
    #[test]
    fn disasm_macro_lists_a_splitkey_cont_chain_and_stops_at_stop() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");

        let args = DisasmArgs {
            mdat,
            smpl,
            macro_number: Some(24),
            pattern: None,
        };
        let mut out = Vec::new();
        run_disasm(&args, &mut out).expect("disasm succeeds on a valid corpus file");
        let text = String::from_utf8(out).expect("output is UTF-8");
        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(lines.len(), 4, "must stop right after the STOP at step 3");
        assert!(lines[0].contains("$1C <Splitkey>"));
        assert!(lines[1].contains("$06 <Cont>"));
        assert!(lines[2].contains("$06 <Cont>"));
        assert!(lines[3].contains("$07 <STOP*>"));
    }

    /// Cross-checked against `tfmx-cli trace`'s own repeated decode of this
    /// step across many sessions (`docs/status.md`): pattern 84 step 0 is
    /// always `Note{note:33, macro:48, volume:12, voice:2, Wait(31)}`.
    #[test]
    fn disasm_pattern_matches_the_known_decode_of_pattern_84_step_0() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");

        let args = DisasmArgs {
            mdat,
            smpl,
            macro_number: None,
            pattern: Some(84),
        };
        let mut out = Vec::new();
        run_disasm(&args, &mut out).expect("disasm succeeds on a valid corpus file");
        let text = String::from_utf8(out).expect("output is UTF-8");

        assert!(text.lines().next().unwrap().contains(
            "Note { note: 33, macro_number: 48, volume: 12, voice: 2, timing: Wait(31) }"
        ));
    }

    #[test]
    fn disasm_requires_exactly_one_of_macro_or_pattern() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");

        let both = DisasmArgs {
            mdat: mdat.clone(),
            smpl: smpl.clone(),
            macro_number: Some(0),
            pattern: Some(0),
        };
        assert!(matches!(
            run_disasm(&both, &mut Vec::new()),
            Err(CliError::Usage(_))
        ));

        let neither = DisasmArgs {
            mdat,
            smpl,
            macro_number: None,
            pattern: None,
        };
        assert!(matches!(
            run_disasm(&neither, &mut Vec::new()),
            Err(CliError::Usage(_))
        ));
    }

    fn write_mono_wav(path: &std::path::Path, samples: &[i16], rate: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    /// Silence, then one loud burst, then silence -- a single onset should
    /// land where the burst starts, not once per window while it sustains.
    #[test]
    fn detect_onsets_finds_one_onset_per_burst() {
        let rate = 44_100;
        let mut samples = vec![0i16; rate as usize / 10]; // 100ms silence
        samples.extend(std::iter::repeat_n(20_000i16, rate as usize / 10)); // 100ms loud
        samples.extend(vec![0i16; rate as usize / 10]); // 100ms silence

        let onsets = detect_onsets(&samples, rate, 20);
        assert_eq!(onsets.len(), 1, "onsets: {onsets:?}");
        assert!(
            (0.08..0.12).contains(&onsets[0]),
            "onset at {} should land near the 100ms burst start",
            onsets[0]
        );
    }

    #[test]
    fn detect_onsets_ignores_low_level_noise() {
        let rate = 44_100;
        // Constant quiet hiss, well below the noise floor -- no onset.
        let samples = vec![10i16; rate as usize / 5];
        let onsets = detect_onsets(&samples, rate, 20);
        assert!(onsets.is_empty(), "onsets: {onsets:?}");
    }

    #[test]
    fn detect_onsets_finds_two_separated_bursts() {
        let rate = 44_100;
        let mut samples = vec![0i16; rate as usize / 10];
        samples.extend(std::iter::repeat_n(20_000i16, rate as usize / 10));
        samples.extend(vec![0i16; rate as usize / 10]);
        samples.extend(std::iter::repeat_n(20_000i16, rate as usize / 10));
        samples.extend(vec![0i16; rate as usize / 10]);

        let onsets = detect_onsets(&samples, rate, 20);
        assert_eq!(onsets.len(), 2, "onsets: {onsets:?}");
    }

    #[test]
    fn pearson_correlation_of_identical_sequences_is_one() {
        let a = [0.10, 0.20, 0.15, 0.30];
        let r = pearson_correlation(&a, &a).expect("enough samples for a correlation");
        assert!((r - 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn pearson_correlation_of_opposite_trends_is_negative() {
        let a = [0.0, 1.0, 2.0, 3.0];
        let b = [3.0, 2.0, 1.0, 0.0];
        let r = pearson_correlation(&a, &b).expect("enough samples for a correlation");
        assert!((r + 1.0).abs() < 1e-9, "r={r}");
    }

    #[test]
    fn pearson_correlation_needs_at_least_two_points() {
        assert_eq!(pearson_correlation(&[1.0], &[1.0]), None);
        assert_eq!(pearson_correlation(&[], &[]), None);
    }

    /// `onset-diff` on two copies of the same synthetic WAV: perfectly
    /// correlated, matching onset counts.
    #[test]
    fn onset_diff_reports_matching_stats_for_identical_input() {
        let rate = 44_100;
        let mut samples = vec![0i16; rate as usize / 10];
        for _ in 0..3 {
            samples.extend(std::iter::repeat_n(20_000i16, rate as usize / 10));
            samples.extend(vec![0i16; rate as usize / 10]);
        }

        let a_path = std::env::temp_dir().join("tfmx-cli-test-onset-a.wav");
        let b_path = std::env::temp_dir().join("tfmx-cli-test-onset-b.wav");
        write_mono_wav(&a_path, &samples, rate);
        write_mono_wav(&b_path, &samples, rate);

        let args = OnsetDiffArgs {
            a: a_path.clone(),
            b: b_path.clone(),
            window_ms: 20,
        };
        let mut out = Vec::new();
        run_onset_diff(&args, &mut out).expect("onset-diff succeeds on valid WAV files");
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("a: 3 onsets"), "{text}");
        assert!(text.contains("b: 3 onsets"), "{text}");
        assert!(text.contains("correlation: 1.000"), "{text}");

        std::fs::remove_file(&a_path).ok();
        std::fs::remove_file(&b_path).ok();
    }
}
