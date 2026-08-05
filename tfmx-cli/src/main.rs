use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tfmx::TraceEvent;

mod export;
mod mermaid;
mod midi;
mod midi_mapping;
mod serialize;
mod visualize;

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
    /// Render a single macro to a WAV file, triggered directly with a given
    /// note/volume -- no trackstep, pattern or track transpose involved.
    /// For comparing this crate's macro playback against an editor's own
    /// macro-audition feature, isolated from any song-context transpose.
    RenderMacro(RenderMacroArgs),
    /// Render a single pattern to a WAV file, run directly against its own
    /// Note/Command entries -- no trackstep or song context, just a fixed
    /// stand-in transpose/tempo (both trackstep-row inputs in a real song).
    /// For isolating a pattern's own behavior (e.g. `$F0` timing) from
    /// multi-track trackstep gating.
    RenderPattern(RenderPatternArgs),
    /// Measure a rendered WAV's fundamental frequency via autocorrelation --
    /// e.g. this crate's `render-macro` output vs. the editor's own
    /// macro-audition, or against the documented `8363 * 2^((note-24)/12)`
    /// note table. For isolating `note_period()`/pitch from every other
    /// layer (trackstep, pattern, macro effects) that could also be wrong.
    MeasurePitch(MeasurePitchArgs),
    /// Dump a song's static walk (reachable patterns/macros, `mdat`/`smpl`
    /// provenance) plus a zone table for every reachable macro -- the
    /// machine-readable module dump `docs/m5-plan.md` Phase 5.4 calls for.
    Dump(DumpArgs),
    /// Export a song to a Standard MIDI File, via a hand-editable JSON
    /// mapping from 5.3's zone tables to MIDI program/drum/drop.
    /// `docs/m5-plan.md` Phase 5.5.
    ExportMidi(ExportMidiArgs),
    /// Batch-render the corpus against a reference player and score onset
    /// timing/pitch agreement per module, writing a tracked JSON metrics
    /// file. `docs/m5-plan.md` Phase 5.6. Regression detection, not a truth
    /// oracle -- see the scoreboard's own `honesty_note` field.
    FidelityScoreboard(FidelityScoreboardArgs),
    /// Export a song's (or one macro's) sampler instruments -- WAV with a
    /// `smpl` loop chunk, SFZ, or a DecentSampler `.dspreset` -- built over
    /// 5.3's zone table. `docs/m5-plan.md` Phase 5.7.
    ExportInstruments(ExportInstrumentsArgs),
    /// Render a song's waveform regions, pattern->macro call graph and
    /// trackstep structure to a single self-contained HTML file.
    /// `docs/m5-plan.md` Phase 5.8.
    Visualize(VisualizeArgs),
}

#[derive(clap::Args)]
struct VisualizeArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(short = 'o', long = "output")]
    output: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
}

#[derive(clap::Args)]
struct ExportMidiArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(short = 'o', long = "output")]
    output: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, default_value_t = 30)]
    seconds: u32,
    /// JSON mapping file: `(macro, note range, velocity range) -> program |
    /// drum note | drop` (docs/m5-plan.md Phase 5.5). If the path doesn't
    /// exist yet, it is auto-drafted from the song's zone tables and
    /// written there for hand-editing before the export runs; omit
    /// entirely to auto-draft in memory only, for a quick one-off export.
    #[arg(long)]
    mapping: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = GateArg::All)]
    gate: GateArg,
}

#[derive(clap::Args)]
struct ExportInstrumentsArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    /// Directory to write the export into; created if missing. All macros'
    /// files share this one directory, prefixed with their macro number, so
    /// nothing collides.
    #[arg(short = 'o', long = "output")]
    output: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(export::FORMAT_NAMES))]
    format: String,
    /// Export only this macro instead of every macro reachable from `song`.
    #[arg(long = "macro")]
    macro_number: Option<u8>,
}

#[derive(clap::Args)]
struct DumpArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, value_enum, default_value_t = DumpFormat::Json)]
    format: DumpFormat,
}

#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
enum DumpFormat {
    Json,
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

#[derive(clap::Args)]
struct RenderMacroArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(short = 'o', long = "output")]
    output: PathBuf,
    /// Macro number to trigger (0-127).
    #[arg(long = "macro")]
    macro_number: u8,
    /// Note to trigger it with, before any macro-internal transpose
    /// (`$08`/`$09`/`$1F`). Accepts a note name as shown in the editor
    /// (`C-3`, `F#0`, `H-2`, `docs/playback-model.md` §4) or a raw note
    /// byte, decimal or hex (`33`, `0x21`, `$21`) -- a raw byte is masked
    /// to its low 6 bits, same as real pattern decoding, so pasting the
    /// editor's byte for a packed pattern record (e.g. `$A1`) works
    /// without doing that arithmetic by hand. Default is `C-3`, this
    /// crate's middle-C anchor.
    #[arg(long, default_value = "C-3", value_parser = parse_note)]
    note: u8,
    #[arg(long, default_value_t = 64)]
    volume: u8,
    /// Which of Paula's 4 voices to render on -- only affects stereo
    /// position (`docs/playback-model.md` §2.1's fixed pan-per-voice), not
    /// the macro's own behaviour.
    #[arg(long, default_value_t = 0)]
    voice: u8,
    /// Jiffy rate: a stored tempo value, same encoding as the header table
    /// (`docs/playback-model.md` §3.2). Only affects effect speeds
    /// (envelope/vibrato/portamento `every` counts, `<Wait>`), not pitch.
    #[arg(long, default_value_t = 0)]
    tempo: u16,
    #[arg(long, default_value_t = 5)]
    seconds: u32,
    #[arg(long, default_value_t = 44_100)]
    rate: u32,
    #[arg(long, default_value_t = 100)]
    separation: u8,
}

#[derive(clap::Args)]
struct RenderPatternArgs {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(short = 'o', long = "output")]
    output: PathBuf,
    /// Pattern number to run (0-127).
    #[arg(long)]
    pattern: u8,
    /// Stand-in for the trackstep row's per-track transpose -- the one
    /// piece of a Note entry a real song context supplies from outside the
    /// pattern itself (`docs/playback-model.md` §7). Constant for the whole
    /// render, unlike a live trackstep line which can change it every jiffy.
    /// Accepts a plain signed decimal (`-24`) or a raw byte as the
    /// trackstep word's low byte shows it (`0xE8`, `$E8`).
    #[arg(long, default_value_t = 0, value_parser = parse_transpose)]
    transpose: i8,
    /// Jiffy rate: same encoding as the header table (`docs/playback-
    /// model.md` §3.2). Only affects effect speeds and `$F3 <Wait>`, not
    /// pitch.
    #[arg(long, default_value_t = 0)]
    tempo: u16,
    #[arg(long, default_value_t = 10)]
    seconds: u32,
    #[arg(long, default_value_t = 44_100)]
    rate: u32,
    #[arg(long, default_value_t = 100)]
    separation: u8,
}

/// Note names by raw table index (`docs/playback-model.md` §4), verbatim
/// from the editor's own note table -- index `n` is the note byte with its
/// low 6 bits equal to `n` (top 2 bits are pattern-record framing, not part
/// of the note).
const NOTE_NAMES: [&str; 64] = [
    "F#0", "G-0", "G#0", "A-0", "A#0", "H-0", "C-1", "C#1", "D-1", "D#1", "E-1", "F-1", "F#1",
    "G-1", "G#1", "A-1", "A#1", "H-1", "C-2", "C#2", "D-2", "D#2", "E-2", "F-2", "F#2", "G-2",
    "G#2", "A-2", "A#2", "H-2", "C-3", "C#3", "D-3", "D#3", "E-3", "F-3", "F#3", "G-3", "G#3",
    "A-3", "A#3", "H-3", "C-4", "C#4", "D-4", "D#4", "E-4", "F-4", "F#3!", "G-3!", "G#3!", "A-3!",
    "A#3!", "H-3!", "C-4!", "C#4!", "D-4!", "D#4!", "E-4!", "F-4!", "!F#!", "!G-!", "!G#!", "!A-!",
];

/// `--note` accepts either a note name (`C-3`, case-insensitive) or a raw
/// byte (decimal, `0x`-hex or `$`-hex), masked to its low 6 bits so the
/// editor's raw packed-record byte can be pasted directly (see
/// `docs/macro-playback-fidelity.md` §6).
fn parse_note(s: &str) -> Result<u8, String> {
    if let Some(index) = NOTE_NAMES.iter().position(|name| name.eq_ignore_ascii_case(s)) {
        return Ok(index as u8);
    }
    let (radix, digits) = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => (16, hex),
        None => match s.strip_prefix('$') {
            Some(hex) => (16, hex),
            None => (10, s),
        },
    };
    let raw = u8::from_str_radix(digits, radix).map_err(|_| {
        format!(
            "'{s}' is not a note name (e.g. \"C-3\") or a byte 0-255 (decimal, 0x.., or $..)"
        )
    })?;
    Ok(raw & 0x3F)
}

/// `--transpose` accepts a plain signed decimal (`-24`, clap's pre-existing
/// behaviour) or a raw byte (`0x`-hex or `$`-hex, e.g. `0xE8`/`$E8`) as the
/// trackstep track word's low byte shows it, cast via `byte as i8`
/// (two's-complement) -- no masking, unlike `--note`: the transpose byte has
/// no top-bit framing to strip (`tfmx/src/sequencer.rs:149-166`).
fn parse_transpose(s: &str) -> Result<i8, String> {
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).or_else(|| s.strip_prefix('$'));
    if let Some(digits) = hex {
        let raw = u8::from_str_radix(digits, 16)
            .map_err(|_| format!("'{s}' is not a raw byte 0x00-0xFF"))?;
        return Ok(raw as i8);
    }
    s.parse::<i8>().map_err(|_| {
        format!("'{s}' is not a signed decimal (-128..127) or a raw byte (0x.. or $..)")
    })
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
    Json,
}

#[derive(clap::Args)]
struct FidelityScoreboardArgs {
    /// Directory holding the corpus (`mdat.<name>`/`smpl.<name>` pairs),
    /// e.g. produced by `testdata/fetch.sh`.
    #[arg(long, default_value = "testdata")]
    testdata_dir: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, default_value_t = 30)]
    seconds: u32,
    /// Reference player invoked as `<player> -f <wav> -t <seconds> <mdat>`.
    #[arg(long, default_value = "uade123")]
    reference_player: String,
    #[arg(short = 'o', long, default_value = "docs/fidelity-scoreboard.json")]
    output: PathBuf,
}

#[derive(clap::Args)]
struct OnsetDiffArgs {
    a: PathBuf,
    b: PathBuf,
    /// Analysis window size in milliseconds.
    #[arg(long, default_value_t = 20)]
    window_ms: u32,
}

#[derive(clap::Args)]
struct MeasurePitchArgs {
    wav: PathBuf,
    /// Seconds to skip before measuring -- avoids the attack transient/DMA
    /// startup click landing inside the analysis window.
    #[arg(long, default_value_t = 0.2)]
    skip_seconds: f64,
    /// How much audio, starting after `--skip-seconds`, to analyze.
    #[arg(long, default_value_t = 0.3)]
    window_seconds: f64,
}

#[derive(Debug)]
enum CliError {
    Io(std::io::Error),
    Wav(hound::Error),
    Parse(tfmx::ParseError),
    Access(tfmx::AccessError),
    Json(serde_json::Error),
    Usage(&'static str),
    Reference(String),
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

impl From<serde_json::Error> for CliError {
    fn from(e: serde_json::Error) -> Self {
        CliError::Json(e)
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "I/O error: {e}"),
            CliError::Wav(e) => write!(f, "WAV error: {e}"),
            CliError::Parse(e) => write!(f, "invalid module: {e:?}"),
            CliError::Access(e) => write!(f, "out-of-range access: {e:?}"),
            CliError::Json(e) => write!(f, "JSON error: {e}"),
            CliError::Usage(msg) => write!(f, "usage error: {msg}"),
            CliError::Reference(msg) => write!(f, "reference player: {msg}"),
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

/// Drives `MacroInterpreter` + `Paula` directly -- no `Sequencer`, no
/// trackstep/pattern layer, no track transpose. Mirrors
/// `Player::render_inner`'s tick-then-mix loop (`tfmx/src/player.rs`) at a
/// single-voice scale, using the same seam `MacroInterpreter`'s own unit
/// tests already drive standalone.
fn run_render_macro(args: &RenderMacroArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let total_frames = args.rate as usize * args.seconds as usize;
    let pcm = tfmx_analysis::render_macro_pcm(
        &module,
        args.macro_number,
        args.note,
        args.volume,
        args.voice,
        args.tempo,
        args.rate,
        args.separation,
        total_frames,
    )?;

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: args.rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&args.output, spec)?;
    for sample in pcm {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

/// Routes one decoded pattern entry to the voice it names -- the same
/// dispatch `Player`'s private `dispatch_pattern_entry` (`tfmx/src/
/// player.rs`) does, reimplemented here against `MacroInterpreter`'s public
/// methods since that function isn't exported. `$FB <PPat>`'s `track`
/// operand is dropped: with only one pattern running there is no second
/// track to jump to, so it's read as "replace the running pattern",
/// covering the common self-loop/chain case but not a real multi-track jump.
fn dispatch_pattern_entry_standalone(
    entry: tfmx::PatternEntry,
    transpose: i8,
    macros: &mut [tfmx::MacroInterpreter; 4],
    paula: &mut tfmx::Paula,
    lock: &mut [u32; 4],
) -> Option<u8> {
    use tfmx::{PatternCommand, PatternEntry, NoteTiming};
    let voice_of = |nibble: u8| (nibble & 0x03) as usize;
    match entry {
        PatternEntry::Note {
            note,
            macro_number,
            volume,
            voice,
            timing,
        } => {
            let voice = voice_of(voice);
            if lock[voice] > 0 {
                return None;
            }
            let detune = match timing {
                NoteTiming::Detune(detune) => detune,
                NoteTiming::Wait(_) | NoteTiming::Portamento(_) => 0,
            };
            macros[voice].note_on(macro_number, note, volume, transpose, detune);
            None
        }
        PatternEntry::Command(command) => match command {
            PatternCommand::KeyUp { voice } => {
                macros[voice_of(voice)].signal_key_up();
                None
            }
            PatternCommand::Vibrato { speed, voice, depth } => {
                macros[voice_of(voice)].start_vibrato(speed, depth as i8);
                None
            }
            PatternCommand::Envelope { amount, speed, voice, target } => {
                macros[voice_of(voice)].start_envelope(amount, speed + 1, target);
                None
            }
            PatternCommand::Portamento { speed, voice, rate } => {
                macros[voice_of(voice)].start_portamento(speed, rate as i8 as i16);
                None
            }
            PatternCommand::Fade { speed, target } => {
                paula.start_master_volume_slide(speed, target);
                None
            }
            PatternCommand::Lock { channel, ticks } => {
                lock[voice_of(channel)] = ticks as u32;
                None
            }
            PatternCommand::PlayPattern { pattern, .. } => Some(pattern),
            // Flow/timing commands (`Loop`/`Jump`/`Wait`/`GoSub`/`Return`/
            // `Nop`) and the halt commands are already applied by
            // `PatternRunner::apply` before `emit` returns here -- nothing
            // voice-facing left to dispatch.
            _ => None,
        },
    }
}

/// Drives one `PatternRunner` + the 4-voice `MacroInterpreter` array +
/// `Paula` directly -- no `Sequencer`, so no trackstep line and no
/// multi-track transpose refresh (`args.transpose` stands in, constant for
/// the whole render). Mirrors `run_jiffy`'s per-jiffy order (pattern step,
/// then macro tick) at single-pattern scale, the same way `run_render_macro`
/// mirrors it at single-voice scale.
fn run_render_pattern(args: &RenderPatternArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let mut runner = tfmx::PatternRunner::new(&module, args.pattern)?;
    let mut macros: [tfmx::MacroInterpreter; 4] = core::array::from_fn(|_| tfmx::MacroInterpreter::new());
    let mut paula = tfmx::Paula::new(args.separation);
    let mut unsupported = tfmx::UnsupportedOps::default();
    let mut lock = [0u32; 4];
    let mut clock = tfmx::TickClock::new(args.tempo);

    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: args.rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&args.output, spec)?;

    let total_frames = args.rate as usize * args.seconds as usize;
    let mut buf = vec![0i16; 4096 * 2];
    let mut frames_left = total_frames;
    let mut error = None;
    while frames_left > 0 && error.is_none() {
        let chunk_frames = frames_left.min(4096);
        let out = &mut buf[..chunk_frames * 2];
        let mut pos = 0usize;
        clock.advance(args.rate, chunk_frames as u32, |tick_due, span_frames| {
            if tick_due && error.is_none() {
                let mut jump = None;
                let step = runner.advance(|_pattern, _step, entry| {
                    if let Some(target) = dispatch_pattern_entry_standalone(
                        entry,
                        args.transpose,
                        &mut macros,
                        &mut paula,
                        &mut lock,
                    ) {
                        jump = Some(target);
                    }
                });
                match step {
                    Ok(()) => {}
                    Err(e) => error = Some(e.into()),
                }
                if let Some(target) = jump {
                    match tfmx::PatternRunner::new(&module, target) {
                        Ok(r) => runner = r,
                        Err(e) => error = Some(e.into()),
                    }
                }
                for remaining in &mut lock {
                    *remaining = remaining.saturating_sub(1);
                }
                for (voice, mac) in macros.iter_mut().enumerate() {
                    if let Err(e) = mac.tick(&module, &mut paula, voice as u8, &mut unsupported, |_| {}) {
                        error = Some(e.into());
                    }
                }
            }
            let start = pos * 2;
            let end = start + span_frames as usize * 2;
            paula.render(module.smpl(), args.rate, &mut out[start..end]);
            pos += span_frames as usize;
        });
        if let Some(e) = error {
            return Err(e);
        }
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

/// Formats one structured disassembly line back to `disasm`'s exact
/// pre-extraction text (macro-opcode name lookup stays here since it's a
/// display concern, not decoded data).
fn format_disasm_line(line: &tfmx_analysis::DisasmLine) -> String {
    match line {
        tfmx_analysis::DisasmLine::Macro {
            step,
            opcode,
            aa,
            bb,
            cc,
        } => format!(
            "{step:4}: ${opcode:02X} <{}> aa=${aa:02X} bb=${bb:02X} cc=${cc:02X}",
            macro_opcode_name(*opcode)
        ),
        tfmx_analysis::DisasmLine::Pattern { step, entry } => format!("{step:4}: {entry:?}"),
    }
}

fn run_disasm(args: &DisasmArgs, out: &mut impl Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let lines = match (args.macro_number, args.pattern) {
        (Some(n), None) => tfmx_analysis::disassemble_macro(&module, n)?,
        (None, Some(n)) => tfmx_analysis::disassemble_pattern(&module, n)?,
        _ => return Err(CliError::Usage("pass exactly one of --macro or --pattern")),
    };
    for line in &lines {
        writeln!(out, "{}", format_disasm_line(line))?;
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
    format: TraceFormat,
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
        match format {
            TraceFormat::Text => write_text_event(e, out)?,
            TraceFormat::Json => serialize::write_json_event(e, out)?,
        }
    }
    Ok(())
}

/// Renders `module`'s `song` for `seconds` at the trace seam
/// (`Player::render_traced`), collecting every `TraceEvent` in order.
/// Shared by `trace` and `export-midi`, the two commands that consume a
/// full state-machine trace rather than just the PCM.
fn render_trace(
    module: &tfmx::Module,
    song: u8,
    seconds: u32,
    gate: GateArg,
) -> Result<Vec<TraceEvent>, CliError> {
    const SAMPLE_RATE: u32 = 44_100;
    const SEPARATION: u8 = 100;
    let mut player = tfmx::Player::new(module, song, SAMPLE_RATE, SEPARATION)?;
    player.set_trackstep_gate(gate.into());

    let mut events = Vec::new();
    let total_frames = SAMPLE_RATE as usize * seconds as usize;
    let mut buf = vec![0i16; 4096 * 2];
    let mut frames_left = total_frames;
    while frames_left > 0 {
        let chunk_frames = frames_left.min(4096);
        player.render_traced(&mut buf[..chunk_frames * 2], |e| events.push(e))?;
        frames_left -= chunk_frames;
    }
    Ok(events)
}

fn run_trace(args: &TraceArgs, out: &mut impl Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let events = render_trace(&module, args.song, args.seconds, args.gate)?;

    write_trace(&events, args.voice, args.track, args.format, out)?;
    Ok(())
}

fn run_visualize(args: &VisualizeArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let view = tfmx_analysis::build_song_view(&module, args.song)?;
    let module_name = args
        .mdat
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("module");
    let html = visualize::render_html(module_name, &view);
    std::fs::write(&args.output, html)?;
    Ok(())
}

fn run_dump(args: &DumpArgs, out: &mut impl Write) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let walk = tfmx_analysis::walk_song(&module, args.song)?;
    let zones: Vec<_> = walk
        .reachable_macros
        .iter()
        .filter_map(|&m| tfmx_analysis::resolve_zones(&module, m).ok())
        .collect();

    match args.format {
        DumpFormat::Json => serialize::write_dump_json(args.song, &walk, &zones, out)?,
    }
    Ok(())
}

/// Loads `args.mapping` if it exists; otherwise auto-drafts one from the
/// song's zone tables (`docs/m5-plan.md` Phase 5.3/5.5) and, if a path was
/// given, writes it there for hand-editing on the next run.
fn load_or_draft_mapping(
    module: &tfmx::Module,
    args: &ExportMidiArgs,
) -> Result<midi_mapping::MidiMapping, CliError> {
    if let Some(path) = &args.mapping
        && path.exists()
    {
        let text = std::fs::read_to_string(path)?;
        return Ok(serde_json::from_str(&text)?);
    }
    let walk = tfmx_analysis::walk_song(module, args.song)?;
    let mapping = midi_mapping::draft_mapping(module, &walk);
    if let Some(path) = &args.mapping {
        std::fs::write(path, serde_json::to_string_pretty(&mapping)?)?;
    }
    Ok(mapping)
}

fn run_export_midi(args: &ExportMidiArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let mapping = load_or_draft_mapping(&module, args)?;
    let trace = render_trace(&module, args.song, args.seconds, args.gate)?;
    let events = midi::build_events(&trace, &mapping);
    let file = std::fs::File::create(&args.output)?;
    midi::write_smf(&events, file)?;
    Ok(())
}

fn run_export_instruments(args: &ExportInstrumentsArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;

    let Some(serializer) = export::by_name(&args.format) else {
        return Err(CliError::Usage("unknown --format: expected wav, sfz, or dspreset"));
    };

    let macros: Vec<u8> = match args.macro_number {
        Some(n) => vec![n],
        None => tfmx_analysis::walk_song(&module, args.song)?
            .reachable_macros
            .into_iter()
            .collect(),
    };

    std::fs::create_dir_all(&args.output)?;
    let mut instruments_written = 0;
    for macro_number in macros {
        let Ok(instrument) = export::build_instrument(&module, macro_number) else {
            continue;
        };
        if instrument.zones.is_empty() {
            continue;
        }
        serializer.serialize(&instrument, &args.output)?;
        instruments_written += 1;
    }
    println!(
        "wrote {instruments_written} instrument(s) as {} to {}",
        serializer.name(),
        args.output.display()
    );
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

/// Flags a voice whose attack or loop sample region reads past the end of
/// `smpl`. `Paula::next_sample`'s `unwrap_or(0)` silently renders an
/// out-of-bounds read as digital zero instead of erroring
/// (`docs/macro-playback-fidelity.md` §5), so a bug like this shows up as
/// unexplained silence, not a crash -- worth a lint finding regardless of
/// what turns out to cause any one instance of it. Takes the event stream
/// separately from `lint()` (rather than a new parameter to it) since only
/// this check needs `smpl`'s length.
fn check_sample_bounds(events: &[TraceEvent], smpl_len: usize) -> Vec<Finding> {
    let mut out_of_bounds = [false; 4];
    for e in events {
        if let TraceEvent::Voice { voice, state } = e {
            if !state.dma_on {
                continue;
            }
            let attack_end = state.start as usize + state.len as usize * 2;
            let loop_end = state.loop_start as usize + state.loop_len as usize * 2;
            if attack_end > smpl_len || loop_end > smpl_len {
                out_of_bounds[(*voice as usize) & 3] = true;
            }
        }
    }
    (0..4)
        .filter(|&v| out_of_bounds[v])
        .map(|v| Finding {
            name: "sample-region-out-of-bounds",
            detail: format!(
                "voice {v}: a requested sample or loop region reads past the end of smpl"
            ),
        })
        .collect()
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

    let mut report = lint(&events, &unsupported, &pcm, SAMPLE_RATE);
    report
        .findings
        .extend(check_sample_bounds(&events, module.smpl().len()));
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

/// Fundamental frequency of `samples` via autocorrelation: the lag (within
/// `[min_freq, max_freq]`'s corresponding range) whose shifted copy
/// correlates best with the original. Works on any dominantly-periodic
/// waveform, not just a clean sine -- unlike counting zero crossings, this
/// doesn't get confused by a raw 8-bit PCM sample's own harmonic content
/// within one playback loop, since it directly measures the loop's repeat
/// period rather than assuming a single crossing pair per cycle.
fn measure_pitch_hz(samples: &[i16], rate: u32, min_freq: f64, max_freq: f64) -> Option<f64> {
    let mean = samples.iter().map(|&s| s as f64).sum::<f64>() / samples.len().max(1) as f64;
    let x: Vec<f64> = samples.iter().map(|&s| s as f64 - mean).collect();

    let min_lag = ((rate as f64 / max_freq).floor() as usize).max(1);
    let max_lag = ((rate as f64 / min_freq).ceil() as usize).min(x.len() / 2);
    if min_lag >= max_lag {
        return None;
    }

    let mut best_lag = min_lag;
    let mut best_corr = f64::MIN;
    for lag in min_lag..=max_lag {
        let corr: f64 = (0..x.len() - lag).map(|i| x[i] * x[i + lag]).sum();
        if corr > best_corr {
            best_corr = corr;
            best_lag = lag;
        }
    }
    Some(rate as f64 / best_lag as f64)
}

fn run_measure_pitch(args: &MeasurePitchArgs, out: &mut impl Write) -> Result<(), CliError> {
    let (mono, rate) = read_wav_mono(&args.wav)?;
    let skip = ((args.skip_seconds * rate as f64) as usize).min(mono.len());
    let window_len = ((args.window_seconds * rate as f64) as usize).max(1);
    let end = (skip + window_len).min(mono.len());
    let window = &mono[skip..end];

    match measure_pitch_hz(window, rate, 50.0, 8000.0) {
        Some(hz) => writeln!(out, "{hz:.2} Hz")?,
        None => writeln!(out, "no periodic signal found in the analysis window")?,
    }
    Ok(())
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

/// The fixed 10-module corpus `testdata/fetch.sh` provides, name-only (no
/// `mdat.`/`smpl.` prefix) -- same list the `tests` module's corpus-loop
/// tests hardcode, kept separate since that one is `#[cfg(test)]`-only.
const CORPUS_MODULES: [&str; 10] = [
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

const FIDELITY_HONESTY_NOTE: &str = "Regression detection only, not a truth \
    oracle: this project's history includes onset/RMS metrics moving while a \
    human ear judged no improvement (docs/m5-session-log.md). A metric that \
    moves is evidence to investigate, not proof of (in)fidelity. \
    our_pitch_hz/reference_pitch_hz are measured over the whole rendered \
    span of a dense polyphonic mix, the same scope this project's own \
    fidelity investigation already found untrustworthy for measure-pitch \
    (autocorrelation collapses toward the shortest allowed lag rather than \
    tracking a real note) -- treat these two fields as noise unless a \
    module happens to be near-monophonic, not as a pitch comparison.";

#[derive(serde::Serialize)]
struct ModuleFidelity {
    module: String,
    /// Pearson correlation of inter-onset intervals between this crate's
    /// render and the reference, `[-1, 1]`; `None` if either side had fewer
    /// than two onsets.
    onset_correlation: Option<f64>,
    our_pitch_hz: Option<f64>,
    reference_pitch_hz: Option<f64>,
}

#[derive(serde::Serialize)]
struct FidelityScoreboard {
    honesty_note: &'static str,
    seconds: u32,
    modules: Vec<ModuleFidelity>,
}

/// Pure metric computation over already-loaded PCM -- kept separate from
/// rendering/subprocess I/O so a deliberate mutation of `ours` can be
/// unit-tested without a reference player or the corpus on disk.
fn compute_module_fidelity(
    module: &str,
    ours: (&[i16], u32),
    reference: (&[i16], u32),
) -> ModuleFidelity {
    let (ours_mono, ours_rate) = ours;
    let (ref_mono, ref_rate) = reference;
    let ioi_ours = inter_onset_intervals(&detect_onsets(ours_mono, ours_rate, 20));
    let ioi_ref = inter_onset_intervals(&detect_onsets(ref_mono, ref_rate, 20));
    ModuleFidelity {
        module: module.to_string(),
        onset_correlation: pearson_correlation(&ioi_ours, &ioi_ref),
        our_pitch_hz: measure_pitch_hz(ours_mono, ours_rate, 50.0, 8000.0),
        reference_pitch_hz: measure_pitch_hz(ref_mono, ref_rate, 50.0, 8000.0),
    }
}

/// Runs the reference player to render `mdat` (its `smpl.*` sibling is
/// found by the player itself, same directory) to `output`.
fn render_reference_wav(
    player: &str,
    mdat_path: &std::path::Path,
    song: u8,
    seconds: u32,
    output: &std::path::Path,
) -> Result<(), CliError> {
    // `uade123` is a real-time Amiga emulator even with `-f`, and prints a
    // running "Playing time position" progress line regardless -- discarded
    // here rather than flooding this tool's own stdout.
    let status = std::process::Command::new(player)
        .arg("-1")
        .args(["-s", &song.to_string()])
        .args(["-t", &seconds.to_string()])
        .arg("-f")
        .arg(output)
        .arg(mdat_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| CliError::Reference(format!("failed to run `{player}`: {e}")))?;
    if !status.success() {
        return Err(CliError::Reference(format!(
            "`{player}` exited with {status} on {}",
            mdat_path.display()
        )));
    }
    Ok(())
}

fn run_fidelity_scoreboard(args: &FidelityScoreboardArgs) -> Result<(), CliError> {
    let tmp = std::env::temp_dir();
    let mut modules = Vec::with_capacity(CORPUS_MODULES.len());
    for name in CORPUS_MODULES {
        let mdat_path = args.testdata_dir.join(format!("mdat.{name}"));
        let smpl_path = args.testdata_dir.join(format!("smpl.{name}"));
        if !mdat_path.exists() || !smpl_path.exists() {
            return Err(CliError::Usage(
                "corpus module missing -- run `sh testdata/fetch.sh` first",
            ));
        }

        let our_wav = tmp.join(format!("tfmx-fidelity-{name}-ours.wav"));
        let ref_wav = tmp.join(format!("tfmx-fidelity-{name}-reference.wav"));

        let mdat = std::fs::read(&mdat_path)?;
        let smpl = std::fs::read(&smpl_path)?;
        let module = tfmx::Module::parse(&mdat, &smpl)?;
        let render_args = RenderArgs {
            mdat: mdat_path.clone(),
            smpl: smpl_path.clone(),
            output: our_wav.clone(),
            song: args.song,
            seconds: args.seconds,
            rate: 44_100,
            separation: 100,
            solo: None,
            mute: Vec::new(),
            stems: false,
            gate: GateArg::All,
        };
        render_to_wav(&module, &render_args, [false; 4], &our_wav)?;
        render_reference_wav(
            &args.reference_player,
            &mdat_path,
            args.song,
            args.seconds,
            &ref_wav,
        )?;

        let ours = read_wav_mono(&our_wav)?;
        let reference = read_wav_mono(&ref_wav)?;
        modules.push(compute_module_fidelity(
            name,
            (&ours.0, ours.1),
            (&reference.0, reference.1),
        ));
    }

    let scoreboard = FidelityScoreboard {
        honesty_note: FIDELITY_HONESTY_NOTE,
        seconds: args.seconds,
        modules,
    };
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(&args.output)?;
    serde_json::to_writer_pretty(&mut out, &scoreboard).map_err(CliError::Json)?;
    writeln!(out)?;

    for m in &scoreboard.modules {
        match m.onset_correlation {
            Some(r) => println!("{}: onset correlation {r:.3}", m.module),
            None => println!("{}: onset correlation n/a", m.module),
        }
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
        Command::RenderMacro(args) => run_render_macro(args),
        Command::RenderPattern(args) => run_render_pattern(args),
        Command::MeasurePitch(args) => run_measure_pitch(args, &mut std::io::stdout().lock()),
        Command::Dump(args) => run_dump(args, &mut std::io::stdout().lock()),
        Command::ExportMidi(args) => run_export_midi(args),
        Command::FidelityScoreboard(args) => run_fidelity_scoreboard(args),
        Command::ExportInstruments(args) => run_export_instruments(args),
        Command::Visualize(args) => run_visualize(args),
    };
    if let Err(e) = result {
        eprintln!("tfmx-cli: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--note` takes a note name exactly as the editor's own table spells
    /// it (`docs/playback-model.md` §4), case-insensitively.
    #[test]
    fn parse_note_accepts_note_names() {
        assert_eq!(parse_note("C-3").unwrap(), 0x1E);
        assert_eq!(parse_note("c-3").unwrap(), 0x1E, "case-insensitive");
        assert_eq!(parse_note("F#0").unwrap(), 0x00);
        assert_eq!(parse_note("H-2").unwrap(), 0x1D);
    }

    /// A plain decimal or hex byte within `0x00-0x3F` passes through
    /// unchanged -- this is the pre-existing behaviour `render-macro`
    /// callers already relied on.
    #[test]
    fn parse_note_accepts_raw_byte_already_in_range() {
        assert_eq!(parse_note("33").unwrap(), 33);
        assert_eq!(parse_note("0x21").unwrap(), 0x21);
        assert_eq!(parse_note("$21").unwrap(), 0x21);
    }

    /// The editor shows a packed pattern-record byte (top 2 bits are
    /// timing framing, not note) -- `docs/macro-playback-fidelity.md` §6:
    /// pasting it raw used to silently mistrigger. `--note` now masks it
    /// to the low 6 bits, same as real pattern decoding.
    #[test]
    fn parse_note_masks_raw_byte_above_0x3f() {
        assert_eq!(parse_note("161").unwrap(), 0x21, "0xA1 & 0x3F");
        assert_eq!(parse_note("0xA1").unwrap(), 0x21);
    }

    #[test]
    fn parse_note_rejects_garbage() {
        assert!(parse_note("not-a-note").is_err());
    }

    /// Plain signed decimal keeps working exactly as clap's default
    /// `FromStr` parsed it before `parse_transpose` existed.
    #[test]
    fn parse_transpose_accepts_signed_decimal() {
        assert_eq!(parse_transpose("-24").unwrap(), -24);
        assert_eq!(parse_transpose("24").unwrap(), 24);
        assert_eq!(parse_transpose("0").unwrap(), 0);
    }

    /// The trackstep word's low byte as the editor/disasm shows it (raw
    /// hex, unsigned) -- `docs/macro-playback-fidelity.md`'s "NEXT UP":
    /// word `$54E8` -> transpose byte `$E8`, which is -24 two's-complement
    /// (`tfmx/src/sequencer.rs:855-862`). No masking, unlike `--note`: the
    /// transpose byte has no top-bit framing to strip.
    #[test]
    fn parse_transpose_accepts_raw_hex_byte_twos_complement() {
        assert_eq!(parse_transpose("0xE8").unwrap(), -24);
        assert_eq!(parse_transpose("$E8").unwrap(), -24);
        assert_eq!(parse_transpose("0x7F").unwrap(), 127);
        assert_eq!(parse_transpose("0x00").unwrap(), 0);
    }

    #[test]
    fn parse_transpose_rejects_garbage() {
        assert!(parse_transpose("not-a-number").is_err());
        assert!(parse_transpose("0x1FF").is_err(), "byte overflow");
    }

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

    /// `render-macro` triggers a macro directly -- no trackstep/pattern
    /// layer -- and produces a WAV of the requested length, same shape as
    /// `render`'s own check above.
    #[test]
    fn render_macro_writes_a_wav_of_the_requested_length() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let mdat_path = std::env::temp_dir().join("tfmx-cli-test-macro-input.mdat");
        let smpl_path = std::env::temp_dir().join("tfmx-cli-test-macro-input.smpl");
        std::fs::write(&mdat_path, &mdat).unwrap();
        std::fs::write(&smpl_path, &smpl).unwrap();
        let output = std::env::temp_dir().join("tfmx-cli-test-macro-output.wav");

        let args = RenderMacroArgs {
            mdat: mdat_path,
            smpl: smpl_path,
            output: output.clone(),
            macro_number: 48,
            note: 33,
            volume: 64,
            voice: 2,
            tempo: 3,
            seconds: 1,
            rate: 44_100,
            separation: 100,
        };
        run_render_macro(&args).expect("render-macro succeeds on a valid corpus file");

        let reader = hound::WavReader::open(&output).expect("output is a valid WAV file");
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 44_100);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(reader.duration(), 44_100, "WAV must hold exactly 1 second");

        std::fs::remove_file(&output).ok();
    }

    fn wav_sha256(path: &std::path::Path) -> String {
        use sha2::{Digest, Sha256};
        let mut reader = hound::WavReader::open(path).expect("output is a valid WAV file");
        let mut hasher = Sha256::new();
        for sample in reader.samples::<i16>() {
            hasher.update(sample.unwrap().to_le_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// G0 (`docs/gui-plan.md`): pins `run_render_macro`'s and
    /// `run_render_pattern`'s exact WAV output before either is extracted
    /// into `tfmx-analysis`, so the extraction has something to fail
    /// against. Not a decode correctness check -- just "did the bytes
    /// change".
    #[test]
    fn render_macro_output_matches_golden_hash() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let mdat_path = std::env::temp_dir().join("tfmx-cli-test-golden-macro-input.mdat");
        let smpl_path = std::env::temp_dir().join("tfmx-cli-test-golden-macro-input.smpl");
        std::fs::write(&mdat_path, &mdat).unwrap();
        std::fs::write(&smpl_path, &smpl).unwrap();
        let output = std::env::temp_dir().join("tfmx-cli-test-golden-macro-output.wav");

        let args = RenderMacroArgs {
            mdat: mdat_path,
            smpl: smpl_path,
            output: output.clone(),
            macro_number: 28,
            note: 33,
            volume: 64,
            voice: 0,
            tempo: 3,
            seconds: 2,
            rate: 44_100,
            separation: 100,
        };
        run_render_macro(&args).expect("render-macro succeeds on a valid corpus file");

        assert_eq!(
            wav_sha256(&output),
            "17dc48c406be2115179f34f44c8602397b696d9c2f442be8d22328446aa5fc11",
            "render-macro output changed -- if intentional, update this hash"
        );

        std::fs::remove_file(&output).ok();
    }

    /// G0 (`docs/gui-plan.md`): see `render_macro_output_matches_golden_hash`.
    #[test]
    fn render_pattern_output_matches_golden_hash() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let mdat_path = std::env::temp_dir().join("tfmx-cli-test-golden-pattern-input.mdat");
        let smpl_path = std::env::temp_dir().join("tfmx-cli-test-golden-pattern-input.smpl");
        std::fs::write(&mdat_path, &mdat).unwrap();
        std::fs::write(&smpl_path, &smpl).unwrap();
        let output = std::env::temp_dir().join("tfmx-cli-test-golden-pattern-output.wav");

        let args = RenderPatternArgs {
            mdat: mdat_path,
            smpl: smpl_path,
            output: output.clone(),
            pattern: 84,
            transpose: 0,
            tempo: 3,
            seconds: 2,
            rate: 44_100,
            separation: 100,
        };
        run_render_pattern(&args).expect("render-pattern succeeds on a valid corpus file");

        assert_eq!(
            wav_sha256(&output),
            "85ba6680397ffbbff0d4d7767330eb06a36606602ed7f3327802cf9c9c1a2ec7",
            "render-pattern output changed -- if intentional, update this hash"
        );

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
        write_trace(&events, None, None, TraceFormat::Text, &mut out).unwrap();
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
        write_trace(&events, None, None, TraceFormat::Text, &mut out).unwrap();
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
        write_trace(&events, None, Some(1), TraceFormat::Text, &mut out).unwrap();
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
        write_trace(&events, Some(2), None, TraceFormat::Text, &mut out).unwrap();
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
        write_trace(&events, None, None, TraceFormat::Text, &mut out).unwrap();
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
        write_trace(&events, None, None, TraceFormat::Text, &mut out).unwrap();
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
        write_trace(&events, None, None, TraceFormat::Text, &mut out).unwrap();
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
    fn check_sample_bounds_flags_an_attack_region_reading_past_the_end_of_smpl() {
        let mut state = tfmx::Voice::default();
        state.start = 100;
        state.len = 50; // words -> 100 bytes, end = 200
        state.dma_on = true;
        let events = vec![TraceEvent::Voice { voice: 2, state }];
        let findings = check_sample_bounds(&events, 150);
        let f = findings
            .iter()
            .find(|f| f.name == "sample-region-out-of-bounds")
            .expect("flags an out-of-bounds attack region");
        assert!(f.detail.contains('2'), "names the voice: {}", f.detail);
    }

    #[test]
    fn check_sample_bounds_flags_a_loop_region_reading_past_the_end_of_smpl() {
        let mut state = tfmx::Voice::default();
        state.start = 0;
        state.len = 10;
        state.loop_start = 100;
        state.loop_len = 50; // words -> 100 bytes, end = 200
        state.dma_on = true;
        let events = vec![TraceEvent::Voice { voice: 1, state }];
        let findings = check_sample_bounds(&events, 150);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].detail.contains('1'), "names the voice: {}", findings[0].detail);
    }

    #[test]
    fn check_sample_bounds_does_not_flag_a_region_within_smpl() {
        let mut state = tfmx::Voice::default();
        state.start = 100;
        state.len = 50;
        state.loop_start = 0;
        state.loop_len = 0;
        state.dma_on = true;
        let events = vec![TraceEvent::Voice { voice: 2, state }];
        assert!(check_sample_bounds(&events, 200).is_empty());
    }

    #[test]
    fn check_sample_bounds_ignores_a_voice_with_dma_off() {
        let mut state = tfmx::Voice::default();
        state.start = 100_000; // way out of bounds, but never actually reads
        state.len = 50;
        state.dma_on = false;
        let events = vec![TraceEvent::Voice { voice: 0, state }];
        assert!(check_sample_bounds(&events, 200).is_empty());
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

    #[test]
    fn write_trace_json_emits_one_valid_json_object_per_line() {
        let events = vec![
            TraceEvent::Jiffy {
                frame: 42,
                line: 5,
                tempo: 6,
                stopped: false,
            },
            TraceEvent::Trigger {
                voice: 1,
                macro_number: 2,
                note: 12,
                volume: 64,
                transpose: 0,
            },
        ];
        let mut out = Vec::new();
        write_trace(&events, None, None, TraceFormat::Json, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let jiffy: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(jiffy["type"], "jiffy");
        assert_eq!(jiffy["frame"], 42);
        let trigger: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(trigger["type"], "trigger");
        assert_eq!(trigger["macro"], 2);
    }

    /// Phase 5.4's roadmap check: `dump` produces valid, re-parseable JSON
    /// with a zone table present, across the whole corpus.
    #[test]
    fn dump_json_is_valid_and_has_zone_tables_across_full_corpus() {
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

            let args = DumpArgs {
                mdat,
                smpl,
                song: 0,
                format: DumpFormat::Json,
            };
            let mut out = Vec::new();
            run_dump(&args, &mut out).unwrap_or_else(|e| panic!("{name}: {e}"));
            let value: serde_json::Value =
                serde_json::from_slice(&out).unwrap_or_else(|e| panic!("{name}: {e}"));
            let zones = value["zones"].as_array().expect("zones is an array");
            assert!(!zones.is_empty(), "{name}: expected at least one zone table");
        }
    }

    /// Phase 5.5's own check: a corpus module exports valid MIDI (parses
    /// back via `midly`) whose note count matches the trace's own
    /// `Trigger` event count -- one `NoteOn` per `Trigger`, per
    /// `midi::build_events`'s contract.
    #[test]
    fn export_midi_produces_valid_midi_matching_trigger_count_across_full_corpus() {
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
            let mdat_bytes = std::fs::read(mdat).unwrap();
            let smpl_bytes = std::fs::read(smpl).unwrap();
            let module = tfmx::Module::parse(&mdat_bytes, &smpl_bytes)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));

            let trace = render_trace(&module, 0, 10, GateArg::All)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let trigger_count =
                trace.iter().filter(|e| matches!(e, TraceEvent::Trigger { .. })).count();

            let walk =
                tfmx_analysis::walk_song(&module, 0).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let mapping = midi_mapping::draft_mapping(&module, &walk);
            let events = midi::build_events(&trace, &mapping);
            let note_on_count =
                events.iter().filter(|e| matches!(e.kind, midi::EventKind::NoteOn { .. })).count();
            assert_eq!(note_on_count, trigger_count, "{name}: one NoteOn per Trigger");

            let mut bytes = Vec::new();
            midi::write_smf(&events, &mut bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            let smf = midly::Smf::parse(&bytes).unwrap_or_else(|e| panic!("{name}: invalid MIDI: {e}"));
            let parsed_note_ons = smf.tracks[0]
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        midly::TrackEventKind::Midi {
                            message: midly::MidiMessage::NoteOn { .. },
                            ..
                        }
                    )
                })
                .count();
            assert_eq!(parsed_note_ons, note_on_count, "{name}: note count survives the round trip");
        }
    }

    /// Phase 5.5's check: editing the mapping changes the output -- dropping
    /// the zones of a macro that actually gets triggered removes its notes.
    #[test]
    fn editing_the_mapping_changes_the_exported_notes() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");
        let mdat_bytes = std::fs::read(mdat).unwrap();
        let smpl_bytes = std::fs::read(smpl).unwrap();
        let module = tfmx::Module::parse(&mdat_bytes, &smpl_bytes).unwrap();

        let trace = render_trace(&module, 0, 10, GateArg::All).unwrap();
        let walk = tfmx_analysis::walk_song(&module, 0).unwrap();
        let mut mapping = midi_mapping::draft_mapping(&module, &walk);

        let before = midi::build_events(&trace, &mapping)
            .iter()
            .filter(|e| matches!(e.kind, midi::EventKind::NoteOn { .. }))
            .count();
        assert!(before > 0, "expected some notes before editing the mapping");

        let triggered_macro = trace
            .iter()
            .find_map(|e| match e {
                TraceEvent::Trigger { macro_number, .. } => Some(*macro_number),
                _ => None,
            })
            .expect("song has at least one Trigger in the first 10s");
        for zone in &mut mapping.macros.get_mut(&triggered_macro).unwrap().zones {
            zone.output = midi_mapping::ZoneOutput::Drop;
        }

        let after = midi::build_events(&trace, &mapping)
            .iter()
            .filter(|e| matches!(e.kind, midi::EventKind::NoteOn { .. }))
            .count();
        assert!(after < before, "dropping a triggered macro's zones must remove notes");
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

    /// Phase G1: the decode itself is now tested against the corpus in
    /// `tfmx-analysis::disasm`'s own tests; this only pins the text
    /// `format_disasm_line` renders from a `DisasmLine`.
    #[test]
    fn format_disasm_line_renders_macro_and_pattern_steps_as_before_extraction() {
        let macro_line = tfmx_analysis::DisasmLine::Macro {
            step: 0,
            opcode: 0x1C,
            aa: 0x05,
            bb: 0x00,
            cc: 0x00,
        };
        assert_eq!(
            format_disasm_line(&macro_line),
            "   0: $1C <Splitkey> aa=$05 bb=$00 cc=$00"
        );

        let pattern_line = tfmx_analysis::DisasmLine::Pattern {
            step: 0,
            entry: tfmx::PatternEntry::Note {
                note: 33,
                macro_number: 48,
                volume: 12,
                voice: 2,
                timing: tfmx::NoteTiming::Wait(31),
            },
        };
        assert_eq!(
            format_disasm_line(&pattern_line),
            "   0: Note { note: 33, macro_number: 48, volume: 12, voice: 2, timing: Wait(31) }"
        );
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

    #[test]
    fn measure_pitch_hz_detects_a_known_sine_frequency() {
        let rate = 44_100;
        let freq = 440.0;
        let samples: Vec<i16> = (0..8_000)
            .map(|i| {
                let t = i as f64 / rate as f64;
                (10_000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16
            })
            .collect();
        let hz = measure_pitch_hz(&samples, rate, 50.0, 8000.0).expect("periodic signal found");
        assert!((hz - freq).abs() < 2.0, "measured {hz} Hz, expected ~{freq} Hz");
    }

    #[test]
    fn measure_pitch_hz_detects_a_repeating_non_sine_period() {
        // A raw 8-bit-PCM-like repeating shape (not a clean sine) -- e.g. a
        // sawtooth-ish waveform with harmonic content within one cycle, the
        // way a real sampled instrument loop looks. Autocorrelation should
        // still lock onto the loop's own repeat period rather than a
        // harmonic of it.
        let rate = 44_100;
        let period_samples = 100; // 441 Hz
        let cycle: Vec<i16> = (0..period_samples)
            .map(|i| ((i * 30_000 / period_samples) as i16) - 15_000)
            .collect();
        let samples: Vec<i16> = cycle.iter().copied().cycle().take(8_000).collect();
        let hz = measure_pitch_hz(&samples, rate, 50.0, 8000.0).expect("periodic signal found");
        let expected = rate as f64 / period_samples as f64;
        assert!(
            (hz - expected).abs() < 2.0,
            "measured {hz} Hz, expected ~{expected} Hz"
        );
    }

    #[test]
    fn measure_pitch_hz_does_not_panic_on_silence() {
        let samples = vec![0i16; 8_000];
        measure_pitch_hz(&samples, 44_100, 50.0, 8000.0);
    }

    #[test]
    fn run_measure_pitch_reports_the_measured_frequency() {
        let rate = 44_100;
        let freq = 440.0;
        let samples: Vec<i16> = (0..8_000)
            .map(|i| {
                let t = i as f64 / rate as f64;
                (10_000.0 * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16
            })
            .collect();
        let path = std::env::temp_dir().join("tfmx-cli-test-measure-pitch.wav");
        write_mono_wav(&path, &samples, rate);

        let args = MeasurePitchArgs {
            wav: path.clone(),
            skip_seconds: 0.0,
            window_seconds: 0.15,
        };
        let mut out = Vec::new();
        run_measure_pitch(&args, &mut out).expect("measure-pitch succeeds on a valid WAV file");
        let text = String::from_utf8(out).unwrap();
        let hz: f64 = text
            .trim()
            .strip_suffix(" Hz")
            .expect("prints a Hz value")
            .parse()
            .expect("Hz value is a number");
        assert!((hz - freq).abs() < 2.0, "{text}");

        std::fs::remove_file(&path).ok();
    }

    /// Builds a mono track: silence up to each onset time, a loud burst,
    /// then silence to the next -- mirrors `detect_onsets_finds_two_
    /// separated_bursts`'s shape but at caller-chosen, possibly irregular
    /// onset times, so tests can control the resulting inter-onset-interval
    /// pattern precisely.
    fn samples_with_bursts_at(rate: u32, onset_times_ms: &[u32], burst_ms: u32) -> Vec<i16> {
        let mut samples = Vec::new();
        let mut t_ms = 0u32;
        for &onset_ms in onset_times_ms {
            let silence_len = ((onset_ms - t_ms) as u64 * rate as u64 / 1000) as usize;
            samples.extend(vec![0i16; silence_len]);
            let burst_len = (burst_ms as u64 * rate as u64 / 1000) as usize;
            samples.extend(std::iter::repeat_n(20_000i16, burst_len));
            t_ms = onset_ms + burst_ms;
        }
        samples.extend(vec![0i16; rate as usize / 10]);
        samples
    }

    /// Phase 5.6's own check: a deliberate known-bad mutation (onsets moved
    /// to a differently-shaped rhythm) must move the onset-correlation
    /// metric, not leave it unchanged.
    #[test]
    fn compute_module_fidelity_mutation_moves_onset_correlation() {
        let rate = 44_100;
        let reference = samples_with_bursts_at(rate, &[100, 300, 700, 1400], 10);
        let matching = reference.clone();
        let mutated = samples_with_bursts_at(rate, &[100, 200, 260, 300], 10);

        let good = compute_module_fidelity("test", (&matching, rate), (&reference, rate));
        let bad = compute_module_fidelity("test", (&mutated, rate), (&reference, rate));

        let good_r = good.onset_correlation.expect("identical rhythms correlate");
        let bad_r = bad.onset_correlation.expect("enough onsets for a correlation");
        assert!((good_r - 1.0).abs() < 1e-6, "identical input: r={good_r}");
        assert!(
            bad_r < good_r - 0.5,
            "mutation should move the metric: good={good_r} bad={bad_r}"
        );
    }

    #[test]
    fn fidelity_scoreboard_serializes_to_valid_json() {
        let scoreboard = FidelityScoreboard {
            honesty_note: FIDELITY_HONESTY_NOTE,
            seconds: 30,
            modules: vec![ModuleFidelity {
                module: "turrican intro".to_string(),
                onset_correlation: Some(0.42),
                our_pitch_hz: Some(440.0),
                reference_pitch_hz: None,
            }],
        };
        let text = serde_json::to_string(&scoreboard).expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["modules"][0]["module"], "turrican intro");
        assert!((value["modules"][0]["onset_correlation"].as_f64().unwrap() - 0.42).abs() < 1e-9);
        assert!(value["modules"][0]["reference_pitch_hz"].is_null());
        assert!(value["honesty_note"].as_str().unwrap().contains("not a truth"));
    }

    /// Step 5.6's roadmap check: the scoreboard runs across the whole
    /// corpus and is committed. Needs both the corpus and `uade123` on
    /// `PATH`; skips (CI-safe) if either is missing.
    #[test]
    fn fidelity_scoreboard_runs_across_full_corpus_without_error() {
        if std::process::Command::new("uade123")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: uade123 not found on PATH");
            return;
        }
        if corpus_path("mdat.turrican intro").is_none() {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        }

        let output = std::env::temp_dir().join("tfmx-cli-test-fidelity-scoreboard.json");
        let args = FidelityScoreboardArgs {
            testdata_dir: PathBuf::from(format!("{}/../testdata", env!("CARGO_MANIFEST_DIR"))),
            song: 0,
            seconds: 3,
            reference_player: "uade123".to_string(),
            output: output.clone(),
        };
        run_fidelity_scoreboard(&args).expect("scoreboard runs across the full corpus");

        let text = std::fs::read_to_string(&output).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let modules = value["modules"].as_array().expect("modules array");
        assert_eq!(modules.len(), CORPUS_MODULES.len());
        for m in modules {
            assert!(m["module"].as_str().is_some());
        }

        std::fs::remove_file(&output).ok();
    }

    #[test]
    fn visualize_runs_across_full_corpus_without_error() {
        let mut ran_any = false;
        for name in CORPUS_MODULES {
            let Some(mdat) = corpus_path(&format!("mdat.{name}")) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            };
            let smpl = corpus_path(&format!("smpl.{name}")).expect("smpl present alongside mdat");

            let output = std::env::temp_dir().join(format!(
                "tfmx-cli-test-visualize-{}.html",
                name.replace(' ', "_")
            ));
            let args = VisualizeArgs {
                mdat,
                smpl,
                output: output.clone(),
                song: 0,
            };
            run_visualize(&args).unwrap_or_else(|e| panic!("{name}: {e}"));

            let html = std::fs::read_to_string(&output).unwrap();
            assert!(html.starts_with("<!doctype html>"), "{name}: not well-formed HTML");
            assert!(html.contains("flowchart LR"), "{name}: no call graph");
            assert!(
                html.contains("<table class=\"trackstep\">"),
                "{name}: no trackstep table"
            );
            std::fs::remove_file(&output).ok();
            ran_any = true;
        }
        assert!(ran_any, "no corpus modules found -- run `sh testdata/fetch.sh`");
    }
}
