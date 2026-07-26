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
    /// Print header text, songs, tempos, layout and unsupported opcodes seen.
    Info(InfoArgs),
    /// Print the state-machine trace of a render: one line per event.
    Trace(TraceArgs),
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
    #[arg(long, default_value_t = 0)]
    song: u8,
    /// How long to run the song for while collecting the unsupported-opcode histogram.
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

    const SAMPLE_RATE: u32 = 44_100;
    const SEPARATION: u8 = 100;
    let mut player = tfmx::Player::new(&module, args.song, SAMPLE_RATE, SEPARATION)?;
    let total_frames = SAMPLE_RATE as usize * args.seconds as usize;
    let mut buf = vec![0i16; 4096 * 2];
    let mut frames_left = total_frames;
    while frames_left > 0 {
        let chunk_frames = frames_left.min(4096);
        player.render(&mut buf[..chunk_frames * 2])?;
        frames_left -= chunk_frames;
    }

    writeln!(out, "Unsupported ops:")?;
    let mut any = false;
    for opcode in 0..=255u8 {
        let count = player.unsupported_ops().get(opcode);
        if count > 0 {
            writeln!(out, "  ${opcode:02X}: {count}")?;
            any = true;
        }
    }
    if !any {
        writeln!(out, "  (none)")?;
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

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Render(args) => run_render(args),
        Command::Info(args) => run_info(args, &mut std::io::stdout().lock()),
        Command::Trace(args) => run_trace(args, &mut std::io::stdout().lock()),
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

        let args = InfoArgs {
            mdat,
            smpl,
            song: 0,
            seconds: 1,
        };
        let mut out = Vec::new();
        run_info(&args, &mut out).expect("info succeeds on a valid corpus file");
        let text = String::from_utf8(out).expect("output is UTF-8");

        assert!(text.contains("(Empty)"));
        assert!(text.contains("Layout: Fixed"));
        assert!(text.contains("0: start=75 end=129 tempo=3"));
        assert!(text.contains("1: start=52 end=74 tempo=120"));
        assert!(text.contains("Unsupported ops:"));
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

            let args = InfoArgs {
                mdat,
                smpl,
                song: 0,
                seconds: 1,
            };
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
