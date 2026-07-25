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

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Render(args) => run_render(args),
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
