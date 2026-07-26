use std::path::PathBuf;

use clap::{Parser, Subcommand};

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

fn run_render(args: &RenderArgs) -> Result<(), CliError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;
    let module = tfmx::Module::parse(&mdat, &smpl)?;
    let mut player = tfmx::Player::new(&module, args.song, args.rate, args.separation)?;

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

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Render(args) => run_render(args),
        Command::Info(args) => run_info(args, &mut std::io::stdout().lock()),
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
}
