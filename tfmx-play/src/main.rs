use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Parser)]
#[command(
    name = "tfmx-play",
    about = "Play a TFMX module through the default audio device"
)]
struct Args {
    mdat: PathBuf,
    smpl: PathBuf,
    #[arg(long, default_value_t = 0)]
    song: u8,
    #[arg(long, default_value_t = 100)]
    separation: u8,
}

#[derive(Debug)]
enum PlayError {
    Io(std::io::Error),
    Parse(tfmx::ParseError),
    Access(tfmx::AccessError),
    NoOutputDevice,
    NoOutputConfig(cpal::DefaultStreamConfigError),
    UnsupportedChannelCount(u16),
    UnsupportedSampleFormat(cpal::SampleFormat),
    BuildStream(cpal::BuildStreamError),
    PlayStream(cpal::PlayStreamError),
}

impl From<std::io::Error> for PlayError {
    fn from(e: std::io::Error) -> Self {
        PlayError::Io(e)
    }
}

impl From<tfmx::ParseError> for PlayError {
    fn from(e: tfmx::ParseError) -> Self {
        PlayError::Parse(e)
    }
}

impl From<tfmx::AccessError> for PlayError {
    fn from(e: tfmx::AccessError) -> Self {
        PlayError::Access(e)
    }
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayError::Io(e) => write!(f, "I/O error: {e}"),
            PlayError::Parse(e) => write!(f, "invalid module: {e:?}"),
            PlayError::Access(e) => write!(f, "out-of-range access: {e:?}"),
            PlayError::NoOutputDevice => write!(f, "no default audio output device"),
            PlayError::NoOutputConfig(e) => write!(f, "no usable output config: {e}"),
            PlayError::UnsupportedChannelCount(n) => {
                write!(
                    f,
                    "output device wants {n} channels, only stereo (2) is supported"
                )
            }
            PlayError::UnsupportedSampleFormat(fmt) => {
                write!(f, "output device wants unsupported sample format {fmt}")
            }
            PlayError::BuildStream(e) => write!(f, "failed to build audio stream: {e}"),
            PlayError::PlayStream(e) => write!(f, "failed to start audio stream: {e}"),
        }
    }
}

impl std::error::Error for PlayError {}

/// Scales a core `i16` PCM sample into the `[-1.0, 1.0]` range `cpal`'s `f32`
/// output format expects -- most backends negotiate `f32` as their default,
/// even though `Player::render()` always produces `i16`.
fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

/// Builds and configures (but does not start) the output stream that drives
/// `player` from `cpal`'s audio callback. `player` must already be built at
/// `config`'s own sample rate -- `Player` handles an arbitrary rate exactly
/// (step 4.1), so there is no resampling to do here, only sample-format
/// conversion for backends that don't hand back raw `i16`.
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    mut player: tfmx::Player<'static>,
) -> Result<cpal::Stream, PlayError> {
    let err_fn = |err| eprintln!("tfmx-play: audio stream error: {err}");
    match sample_format {
        cpal::SampleFormat::I16 => device
            .build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    if player.render(data).is_err() {
                        data.fill(0);
                    }
                },
                err_fn,
                None,
            )
            .map_err(PlayError::BuildStream),
        cpal::SampleFormat::F32 => {
            // ponytail: `scratch` grows via `resize` on the first few
            // callbacks and then stays put (cpal keeps a constant buffer
            // length in practice) -- not allocation-free, but the audio
            // core (`tfmx`) itself still is; only this glue layer isn't.
            let mut scratch: Vec<i16> = Vec::new();
            device
                .build_output_stream(
                    config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        scratch.resize(data.len(), 0);
                        if player.render(&mut scratch).is_err() {
                            scratch.fill(0);
                        }
                        for (out, &sample) in data.iter_mut().zip(scratch.iter()) {
                            *out = i16_to_f32(sample);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(PlayError::BuildStream)
        }
        other => Err(PlayError::UnsupportedSampleFormat(other)),
    }
}

fn run(args: &Args) -> Result<(), PlayError> {
    let mdat = std::fs::read(&args.mdat)?;
    let smpl = std::fs::read(&args.smpl)?;

    // The audio callback `cpal` drives must be `'static` (it can run for as
    // long as the stream lives, independent of `run`'s own stack frame).
    // `tfmx-play` is a single-shot, process-lifetime player -- these buffers
    // and the `Module`/`Player` borrowing them are meant to live until exit
    // anyway, so leaking them is the deliberate, documented simplification
    // rather than threading a scoped-thread or `Arc` workaround through
    // `Player`'s borrow-based design.
    let mdat: &'static [u8] = mdat.leak();
    let smpl: &'static [u8] = smpl.leak();
    let module: &'static tfmx::Module<'static> =
        Box::leak(Box::new(tfmx::Module::parse(mdat, smpl)?));

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(PlayError::NoOutputDevice)?;
    let config = device
        .default_output_config()
        .map_err(PlayError::NoOutputConfig)?;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    if stream_config.channels != 2 {
        return Err(PlayError::UnsupportedChannelCount(stream_config.channels));
    }

    let player = tfmx::Player::new(
        module,
        args.song,
        stream_config.sample_rate.0,
        args.separation,
    )?;

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::SeqCst))
            .expect("failed to install Ctrl+C handler");
    }

    let stream = build_stream(&device, &stream_config, sample_format, player)?;
    stream.play().map_err(PlayError::PlayStream)?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}

fn main() {
    let args = Args::parse();
    if let Err(e) = run(&args) {
        eprintln!("tfmx-play: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_song_zero_and_separation_hundred() {
        let args = Args::try_parse_from(["tfmx-play", "a.mdat", "a.smpl"]).unwrap();
        assert_eq!(args.song, 0);
        assert_eq!(args.separation, 100);
    }

    #[test]
    fn parses_song_and_separation_flags() {
        let args = Args::try_parse_from([
            "tfmx-play",
            "a.mdat",
            "a.smpl",
            "--song",
            "5",
            "--separation",
            "50",
        ])
        .unwrap();
        assert_eq!(args.song, 5);
        assert_eq!(args.separation, 50);
    }

    #[test]
    fn i16_to_f32_maps_zero_and_extremes() {
        assert_eq!(i16_to_f32(0), 0.0);
        assert_eq!(i16_to_f32(i16::MIN), -1.0);
        assert!((i16_to_f32(i16::MAX) - 1.0).abs() < 0.001);
    }
}
