use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::cursor::MoveToColumn;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{Clear, ClearType};

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

/// How many subsong slots a module's header table always has
/// (`docs/format.md` §2.2) -- `n`/`p` wrap within this range regardless of
/// how many slots the loaded module actually uses for real songs.
const SONG_SLOTS: u8 = 32;

/// A transport command from the terminal-key reader thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    TogglePause,
    NextSong,
    PrevSong,
    Quit,
}

/// The play/pause/song-selection state machine, kept independent of any
/// real terminal or audio device so it can be unit-tested directly. The
/// audio callback is the only thing that ever applies commands to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Transport {
    song: u8,
    paused: bool,
    running: bool,
}

impl Transport {
    fn new(song: u8) -> Self {
        Self {
            song,
            paused: false,
            running: true,
        }
    }

    /// Applies one command, returning whether `song` changed (the caller's
    /// signal to rebuild `Player` for the new subsong).
    fn apply(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::TogglePause => {
                self.paused = !self.paused;
                false
            }
            Command::NextSong => {
                self.song = (self.song + 1) % SONG_SLOTS;
                true
            }
            Command::PrevSong => {
                self.song = (self.song + SONG_SLOTS - 1) % SONG_SLOTS;
                true
            }
            Command::Quit => {
                self.running = false;
                false
            }
        }
    }
}

/// Everything the audio callback owns: the transport state machine, the
/// currently-playing `Player`, and what it needs to rebuild `Player` on a
/// song switch. `apply_pending_commands` is cheap and allocation-free
/// (`Player::new` never allocates), safe to call on every callback.
struct Realtime {
    module: &'static tfmx::Module<'static>,
    sample_rate: u32,
    separation: u8,
    transport: Transport,
    player: tfmx::Player<'static>,
    commands: mpsc::Receiver<Command>,
    running: Arc<AtomicBool>,
}

impl Realtime {
    fn new(
        module: &'static tfmx::Module<'static>,
        song: u8,
        sample_rate: u32,
        separation: u8,
        commands: mpsc::Receiver<Command>,
        running: Arc<AtomicBool>,
    ) -> Result<Self, PlayError> {
        let player = tfmx::Player::new(module, song, sample_rate, separation)?;
        Ok(Self {
            module,
            sample_rate,
            separation,
            transport: Transport::new(song),
            player,
            commands,
            running,
        })
    }

    fn apply_pending_commands(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            if self.transport.apply(cmd) {
                self.player = tfmx::Player::new(
                    self.module,
                    self.transport.song,
                    self.sample_rate,
                    self.separation,
                )
                .expect("song is always 0..SONG_SLOTS, always in range");
            }
            if !self.transport.running {
                self.running.store(false, Ordering::SeqCst);
            }
        }
    }

    fn silent(&self) -> bool {
        self.transport.paused || !self.transport.running
    }
}

/// Builds and configures (but does not start) the output stream that drives
/// `state` from `cpal`'s audio callback. `state.player` must already be
/// built at `config`'s own sample rate -- `Player` handles an arbitrary rate
/// exactly (step 4.1), so there is no resampling to do here, only
/// sample-format conversion for backends that don't hand back raw `i16`.
fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    mut state: Realtime,
) -> Result<cpal::Stream, PlayError> {
    let err_fn = |err| eprintln!("tfmx-play: audio stream error: {err}");
    match sample_format {
        cpal::SampleFormat::I16 => device
            .build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    state.apply_pending_commands();
                    // Short-circuits: `render` never runs while silent, so
                    // a pause genuinely freezes the player's jiffy clock
                    // rather than advancing it under muted output.
                    if state.silent() || state.player.render(data).is_err() {
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
                        state.apply_pending_commands();
                        scratch.resize(data.len(), 0);
                        // Short-circuits: see the `I16` branch above.
                        if state.silent() || state.player.render(&mut scratch).is_err() {
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

/// Reads raw terminal key events on a dedicated thread and forwards
/// transport commands until `q`/Ctrl+C is pressed or the terminal closes.
/// Must run after `crossterm::terminal::enable_raw_mode()` so keys arrive
/// one at a time instead of line-buffered.
///
/// Also prints the resulting song/pause state after every command, as a
/// single line overwritten in place (`MoveToColumn` + `Clear`, no bare `\n`)
/// -- raw mode turns off the terminal's normal LF-&gt;CRLF translation, so a
/// plain `eprintln!` only moves down, not back to column 0, and each update
/// would stair-step further right than the last. This thread is the sole
/// producer on `tx`, so a second `Transport` mirrored here purely for
/// display can never drift from the audio callback's authoritative one --
/// both apply the exact same commands in the exact same order, just on
/// different copies of the same pure state machine. Printing here rather
/// than from the audio callback keeps this I/O off the realtime thread.
fn spawn_key_reader(tx: mpsc::Sender<Command>, initial_song: u8) {
    std::thread::spawn(move || {
        let mut display = Transport::new(initial_song);
        loop {
            let event = match crossterm::event::read() {
                Ok(event) => event,
                Err(_) => return,
            };
            let Event::Key(key) = event else { continue };
            let cmd = match key.code {
                KeyCode::Char(' ') => Command::TogglePause,
                KeyCode::Char('n') | KeyCode::Char('N') => Command::NextSong,
                KeyCode::Char('p') | KeyCode::Char('P') => Command::PrevSong,
                KeyCode::Char('q') | KeyCode::Char('Q') => Command::Quit,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Command::Quit
                }
                _ => continue,
            };
            let quit = cmd == Command::Quit;
            display.apply(cmd);
            if !quit {
                let status = format!(
                    "tfmx-play: song {}{}",
                    display.song,
                    if display.paused { " (paused)" } else { "" }
                );
                let _ = execute!(
                    std::io::stderr(),
                    MoveToColumn(0),
                    Clear(ClearType::CurrentLine),
                    Print(status),
                );
            }
            if tx.send(cmd).is_err() || quit {
                return;
            }
        }
    });
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

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::SeqCst))
            .expect("failed to install Ctrl+C handler");
    }

    let (tx, rx) = mpsc::channel();
    let state = Realtime::new(
        module,
        args.song,
        stream_config.sample_rate.0,
        args.separation,
        rx,
        Arc::clone(&running),
    )?;

    // Every fallible setup step runs before raw mode is enabled, so an
    // error here never leaves the terminal in raw mode with nothing left to
    // restore it.
    let stream = build_stream(&device, &stream_config, sample_format, state)?;
    stream.play().map_err(PlayError::PlayStream)?;

    // Printed before raw mode is enabled, so its trailing newline still gets
    // the terminal's normal LF->CRLF translation -- everything printed
    // after this point uses explicit `\r`/`MoveToColumn` instead (see
    // `spawn_key_reader`).
    eprintln!(
        "tfmx-play: playing song {} -- space=pause  n/p=song  q=quit",
        args.song
    );

    crossterm::terminal::enable_raw_mode()?;
    spawn_key_reader(tx, args.song);

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // The status line above never ends in a real newline (it's overwritten
    // in place); without one here the shell's next prompt would land on the
    // same line right after it.
    eprintln!();
    crossterm::terminal::disable_raw_mode().ok();
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

    // -- Transport: play/pause/song-selection state machine --

    #[test]
    fn starts_unpaused_and_running_at_the_given_song() {
        let t = Transport::new(3);
        assert_eq!(t.song, 3);
        assert!(!t.paused);
        assert!(t.running);
    }

    #[test]
    fn toggle_pause_flips_and_reports_no_song_change() {
        let mut t = Transport::new(0);
        assert!(!t.apply(Command::TogglePause));
        assert!(t.paused);
        assert!(!t.apply(Command::TogglePause));
        assert!(!t.paused);
    }

    #[test]
    fn next_and_prev_song_report_a_song_change() {
        let mut t = Transport::new(5);
        assert!(t.apply(Command::NextSong));
        assert_eq!(t.song, 6);
        assert!(t.apply(Command::PrevSong));
        assert_eq!(t.song, 5);
    }

    #[test]
    fn next_song_wraps_past_the_last_slot() {
        let mut t = Transport::new(SONG_SLOTS - 1);
        t.apply(Command::NextSong);
        assert_eq!(t.song, 0);
    }

    #[test]
    fn prev_song_wraps_before_the_first_slot() {
        let mut t = Transport::new(0);
        t.apply(Command::PrevSong);
        assert_eq!(t.song, SONG_SLOTS - 1);
    }

    #[test]
    fn quit_stops_running_and_reports_no_song_change() {
        let mut t = Transport::new(0);
        assert!(!t.apply(Command::Quit));
        assert!(!t.running);
    }

    #[test]
    fn pause_survives_a_song_change_and_vice_versa() {
        let mut t = Transport::new(0);
        t.apply(Command::TogglePause);
        t.apply(Command::NextSong);
        assert!(t.paused, "song switch must not implicitly resume");
        assert_eq!(t.song, 1);
    }
}
