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
}

#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
enum TraceFormat {
    Text,
}

#[derive(Debug)]
enum CliError {
    Io(std::io::Error),
    Wav(hound::Error),
    Parse(tfmx::ParseError),
    Access(tfmx::AccessError),
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

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Render(args) => run_render(args),
        Command::Info(args) => run_info(args, &mut std::io::stdout().lock()),
        Command::Trace(args) => run_trace(args, &mut std::io::stdout().lock()),
        Command::Lint(args) => run_lint(args, &mut std::io::stdout().lock()),
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
            },
            &mut song1,
        )
        .expect("trace succeeds on a valid corpus file");

        assert_ne!(song0, song1);
    }
}
