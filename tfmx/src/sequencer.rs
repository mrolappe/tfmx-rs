//! Tick scheduling and the trackstep runner.
//!
//! [`TickClock`]: the jiffy clock. See `docs/architecture.md` §2: tempo (the
//! tick-rate fraction) and phase (the sub-sample accumulator) are owned
//! here, apart from the mixer, which is what makes block-size independence a
//! property of this type rather than of any single `render()` call.
//!
//! [`Sequencer`]: the trackstep runner (step 4.2). See `docs/format.md` §5
//! and `docs/opcodes.md` §1.

use crate::module::{AccessError, Module};

/// The jiffy clock: derives the tick rate from a tempo-table value and
/// schedules tick boundaries at exact sample positions.
///
/// A tick boundary lands at the mathematically exact sample index over an
/// arbitrarily long render, because the tick length is kept as the integer
/// fraction `num/den` and its remainder is carried in `acc` instead of being
/// rounded per tick. `samples_per_tick` is commonly non-integer (44100 Hz at
/// BPM 140 is 787.5), so rounding would compound into audible drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickClock {
    /// Tempo-table value; see [`tick_fraction`] for how it becomes a rate.
    tempo: u16,
    /// Remainder of the tick length not yet paid out, in units of `den`.
    acc: u64,
    /// Whole samples remaining before the next tick. 0 = a tick is due.
    next_boundary_offset: u32,
}

/// Samples per tick as the exact fraction `(num, den)`, never a float.
///
/// The tempo value selects the derivation purely by magnitude
/// (`docs/playback-model.md`): `v <= 15` is the 50 Hz-divider path with
/// `tick_rate_hz = 50 / (v + 1)`, `v > 15` is the CIA/BPM path with
/// `tick_rate_hz = v * 24 / 60`. The two coincide exactly at divisor 0 /
/// BPM 125, and both paths schedule identically there.
pub fn tick_fraction(tempo: u16, sample_rate: u32) -> (u64, u64) {
    let sample_rate = u64::from(sample_rate);
    if tempo <= 15 {
        (sample_rate * (u64::from(tempo) + 1), 50)
    } else {
        (sample_rate * 60, u64::from(tempo) * 24)
    }
}

impl TickClock {
    /// A clock at tempo `tempo` with its first tick due immediately.
    pub fn new(tempo: u16) -> Self {
        Self {
            tempo,
            acc: 0,
            next_boundary_offset: 0,
        }
    }

    /// Sets the tempo, leaving the phase untouched: the sub-sample remainder
    /// carries across the change rather than resetting. The tick already
    /// scheduled keeps its length; the new tempo takes effect from the next.
    pub fn set_tempo(&mut self, tempo: u16) {
        self.tempo = tempo;
    }

    pub fn tempo(&self) -> u16 {
        self.tempo
    }

    /// Whole samples remaining before the next tick; 0 means one is due now.
    /// A query — it advances nothing.
    pub fn samples_until_next_tick(&self, _sample_rate: u32) -> u32 {
        self.next_boundary_offset
    }

    /// Advances `frames` samples of output, calling `span(tick_due, frames)`
    /// once per run of samples over which the register state is constant.
    /// `tick_due` is true when a jiffy tick falls on the run's first sample,
    /// i.e. when the caller must advance tick-driven state before
    /// synthesizing.
    ///
    /// Splitting one render request into several sequential calls yields the
    /// same tick positions as one call for the total, at any chunking.
    pub fn advance(&mut self, sample_rate: u32, frames: u32, mut span: impl FnMut(bool, u32)) {
        let (num, den) = tick_fraction(self.tempo, sample_rate);
        let mut pos = 0;
        while pos < frames {
            let tick_due = self.next_boundary_offset == 0;
            if tick_due {
                self.acc += num;
                // ponytail: clamped to one sample per tick so a degenerate
                // tempo (tick shorter than a sample) cannot spin forever.
                let step = (self.acc / den).max(1);
                self.acc = self.acc.saturating_sub(step * den);
                self.next_boundary_offset = step as u32;
            }
            let chunk = (frames - pos).min(self.next_boundary_offset);
            span(tick_due, chunk);
            pos += chunk;
            self.next_boundary_offset -= chunk;
        }
    }
}

/// One decoded trackstep word for a single track. `docs/format.md` §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSlot {
    /// Start pattern `number`, transposed by `transpose` semitones.
    Pattern { number: u8, transpose: i8 },
    /// Keep the currently running pattern; only the transpose changes.
    /// "`$80` hold still applies transpose" — `docs/playback-model.md` §6.
    Hold { transpose: i8 },
    /// Stop this track.
    StopChannel,
    /// Stop the voice named by `voice` (independent of this track).
    StopVoice { voice: u8 },
}

/// A decoded `$EFFE` trackstep line command. `docs/opcodes.md` §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCommand {
    /// Stops the player.
    Stop,
    /// Plays the section `[position, current line]`, `times` times.
    /// `times == 0` repeats indefinitely.
    PlaySection { position: u16, times: u16 },
    /// Sets playback tempo. `cia_bpm == 0xFFFF` means "no change"; see
    /// `docs/playback-model.md` §3.3 for the (unstated by [S1]) precedence
    /// this crate applies when both fields are otherwise meaningful.
    SetTempo { divisor: u16, cia_bpm: u16 },
    /// Starts a master-volume slide toward `target`, one step every
    /// `divisor` jiffies. [S1] never states what distinguishes this from
    /// [`LineCommand::MasterVolSlideB`] — `docs/opcodes.md` §1.
    MasterVolSlideA { divisor: u16, target: u16 },
    /// Same stated effect as [`LineCommand::MasterVolSlideA`]; see there.
    MasterVolSlideB { divisor: u16, target: u16 },
    /// A command number [S1] does not document. Recorded, never guessed.
    Unknown { opcode: u16 },
}

/// One decoded trackstep line: either one word per track, or a `$EFFE`
/// command spanning the whole line. `docs/format.md` §5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackstepLine {
    Tracks([TrackSlot; 8]),
    Command(LineCommand),
}

const EFFE: u16 = 0xEFFE;

fn decode_track_word(word: u16) -> TrackSlot {
    let hi = (word >> 8) as u8;
    let lo = word as u8;
    match hi {
        0x80 => TrackSlot::Hold {
            transpose: lo as i8,
        },
        0xFE => TrackSlot::StopVoice { voice: lo },
        0xFF => TrackSlot::StopChannel,
        // `docs/format.md` §5 only documents $00-$7F as a pattern number;
        // $81-$FD is unstated. Masked rather than rejected, matching the
        // voice-nibble tolerance in `Player::voice_of` -- 128 patterns only
        // ever need 7 bits, so a stray top bit is dropped instead of
        // erroring out the whole render (see the test for the real corpus
        // word this comes from).
        number => TrackSlot::Pattern {
            number: number & 0x7F,
            transpose: lo as i8,
        },
    }
}

fn word_at(bytes: &[u8; 16], i: usize) -> u16 {
    u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]])
}

fn decode_line(bytes: &[u8; 16]) -> TrackstepLine {
    if word_at(bytes, 0) != EFFE {
        let mut slots = [TrackSlot::StopChannel; 8];
        for (i, slot) in slots.iter_mut().enumerate() {
            *slot = decode_track_word(word_at(bytes, i));
        }
        return TrackstepLine::Tracks(slots);
    }

    let param_a = word_at(bytes, 2);
    let param_b = word_at(bytes, 3);
    let command = match word_at(bytes, 1) {
        0x0000 => LineCommand::Stop,
        0x0001 => LineCommand::PlaySection {
            position: param_a,
            times: param_b,
        },
        0x0002 => LineCommand::SetTempo {
            divisor: param_a,
            cia_bpm: param_b,
        },
        0x0003 => LineCommand::MasterVolSlideA {
            divisor: param_a,
            target: param_b,
        },
        0x0004 => LineCommand::MasterVolSlideB {
            divisor: param_a,
            target: param_b,
        },
        opcode => LineCommand::Unknown { opcode },
    };
    TrackstepLine::Command(command)
}

/// Additional repeats remaining for a `PlaySection` currently in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionRepeat {
    Finite(u16),
    Infinite,
}

/// The trackstep runner: the current line, each track's loaded
/// pattern/transpose, and the tick clock derived from the song's tempo
/// slot. `docs/playback-model.md` §1 and §3; `docs/format.md` §5.
///
/// This does not yet drive a [`crate::Paula`] or decode patterns (steps 4.3
/// and 4.4) -- [`Sequencer::advance`] is an explicit, externally-triggered
/// step to one trackstep line, since the trackstep record itself carries no
/// notion of *when* to advance (`docs/opcodes.md` ties that to a pattern's
/// `$F0 <End>`, which does not exist here yet).
#[derive(Debug)]
pub struct Sequencer<'a> {
    module: &'a Module<'a>,
    clock: TickClock,
    line: u16,
    song_start: u16,
    song_end: u16,
    tracks: [TrackSlot; 8],
    section_repeat: Option<SectionRepeat>,
    stopped: bool,
}

impl<'a> Sequencer<'a> {
    /// A sequencer positioned at the start of song `song` (0-31), with the
    /// tempo from that song's header-table slot.
    pub fn new(module: &'a Module<'a>, song: u8) -> Result<Self, AccessError> {
        if song >= 32 {
            return Err(AccessError::OutOfRange);
        }
        let song_start = module.song_start(song);
        Ok(Self {
            module,
            clock: TickClock::new(module.tempo(song)),
            line: song_start,
            song_start,
            song_end: module.song_end(song),
            tracks: [TrackSlot::StopChannel; 8],
            section_repeat: None,
            stopped: false,
        })
    }

    /// The trackstep line about to be (or last) processed.
    pub fn current_line(&self) -> u16 {
        self.line
    }

    /// Track `track`'s (0-7) currently loaded pattern/transpose state.
    pub fn track(&self, track: u8) -> TrackSlot {
        self.tracks[track as usize]
    }

    /// Whether `$EFFE 0000 Stop` has halted the player.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// The active tempo-table value (`docs/playback-model.md` §3.2).
    pub fn tempo(&self) -> u16 {
        self.clock.tempo()
    }

    /// Processes the current trackstep line and moves `self.line` to the
    /// next one (or wherever the line's command redirects to), returning
    /// the line as decoded. A no-op once [`Sequencer::is_stopped`].
    pub fn advance(&mut self) -> Result<TrackstepLine, AccessError> {
        // The header's 32-slot song table doesn't guarantee every slot is
        // actually used by the module -- an unused/placeholder slot's
        // start/end can point past the real trackstep table. That is the
        // same situation an authored `$EFFE 0000 Stop` line describes
        // (nothing further to play), not a data-integrity error.
        let Ok(bytes) = self.module.trackstep_line(self.line) else {
            self.stopped = true;
            return Ok(TrackstepLine::Command(LineCommand::Stop));
        };
        let decoded = decode_line(bytes);
        if self.stopped {
            return Ok(decoded);
        }
        match &decoded {
            TrackstepLine::Tracks(slots) => {
                for (track, slot) in self.tracks.iter_mut().zip(slots.iter()) {
                    *track = match (*track, *slot) {
                        (TrackSlot::Pattern { number, .. }, TrackSlot::Hold { transpose }) => {
                            TrackSlot::Pattern { number, transpose }
                        }
                        (_, new_slot) => new_slot,
                    };
                }
                self.step_line();
            }
            TrackstepLine::Command(command) => self.apply_command(*command),
        }
        Ok(decoded)
    }

    /// Moves to the next line, looping back to `song_start` once past
    /// `song_end` -- the default "song start/end/loop" behavior for a line
    /// that does not itself redirect flow.
    fn step_line(&mut self) {
        self.line += 1;
        if self.line > self.song_end {
            self.line = self.song_start;
        }
    }

    fn apply_command(&mut self, command: LineCommand) {
        match command {
            LineCommand::Stop => self.stopped = true,
            LineCommand::SetTempo { divisor, cia_bpm } => {
                let tempo = if cia_bpm != 0xFFFF { cia_bpm } else { divisor };
                self.clock.set_tempo(tempo);
                self.step_line();
            }
            LineCommand::PlaySection { position, times } => {
                let repeat = *self.section_repeat.get_or_insert(if times == 0 {
                    SectionRepeat::Infinite
                } else {
                    SectionRepeat::Finite(times)
                });
                match repeat {
                    SectionRepeat::Infinite => self.line = position,
                    SectionRepeat::Finite(0) => {
                        self.section_repeat = None;
                        self.step_line();
                    }
                    SectionRepeat::Finite(n) => {
                        self.section_repeat = Some(SectionRepeat::Finite(n - 1));
                        self.line = position;
                    }
                }
            }
            // Nothing in this crate yet consumes a master-volume slide
            // (there is no master-volume concept on `Paula`); recognized
            // and timed like any other line command, executed by a later
            // step once there is somewhere to apply it.
            LineCommand::MasterVolSlideA { .. }
            | LineCommand::MasterVolSlideB { .. }
            | LineCommand::Unknown { .. } => self.step_line(),
        }
    }
}

/// What a note longword's last byte means, selected by the top two bits of
/// its first byte. `docs/format.md` §6, `docs/opcodes.md` §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteTiming {
    /// First byte `$00`-`$7F`: a finetune value. The note does not wait --
    /// the next entry is fetched in the same jiffy.
    Detune(i8),
    /// First byte `$80`-`$BF`: the next entry is fetched `wait` + 1 jiffies
    /// later.
    Wait(u8),
    /// First byte `$C0`-`$EF`: the note is reached by portamento from the
    /// previous note at this rate (as `$FC`) instead of being played
    /// directly. Like [`NoteTiming::Detune`] it does not wait.
    Portamento(u8),
}

/// One decoded pattern longword. `docs/format.md` §6, `docs/opcodes.md` §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternEntry {
    /// A note `aa bb cv dd`: trigger macro `macro_number` on voice `voice`
    /// for `note` at relative volume `volume`.
    ///
    /// Only the low 6 bits of `aa` select the note, so `note` is `$00`-`$3F`
    /// and the discarded top bits live on in `timing`. `macro_number` is the
    /// whole of `bb` (the corpus does use macro numbers above `$3F`).
    /// **Uncertain**: [S1] names `v` but never explains it; `docs/format.md`
    /// §6 records that, and this crate reads it as the voice the macro runs
    /// on, per `docs/opcodes.md` §2.
    Note {
        note: u8,
        macro_number: u8,
        volume: u8,
        voice: u8,
        timing: NoteTiming,
    },
    /// A `$F0`-`$FF` command longword.
    Command(PatternCommand),
}

/// A decoded pattern command (`$F0`-`$FF`). `docs/opcodes.md` §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternCommand {
    /// `$F0 <End>`: ends this pattern; the trackstep advances.
    End,
    /// `$F1 <Loop>`: repeats the block from longword step `target` up to
    /// (not including) this command, `times` times; `times == 0` repeats
    /// indefinitely. `target` is a step index relative to this pattern, not
    /// a byte offset -- see [`PatternRunner`].
    Loop { times: u8, target: u16 },
    /// `$F2 <Jump>`: continues in pattern `pattern` at longword step `step`.
    Jump { pattern: u8, step: u16 },
    /// `$F3 <Wait>`: waits `jiffies` + 1 jiffies.
    Wait { jiffies: u8 },
    /// `$F4 <STOP>`: stops this track, unrecoverably until a new pattern is
    /// loaded; a pending `<End>` never runs.
    Stop,
    /// `$F5 <Kup^>`: sets the release flag on voice `voice`.
    KeyUp { voice: u8 },
    /// `$F6 <Vibr>`: macro `$0C <Vibrato>` on voice `voice`; `2 * speed` is
    /// the waveform period in jiffies, `depth` the per-jiffy period slide.
    Vibrato { speed: u8, voice: u8, depth: u8 },
    /// `$F7 <Enve>`: every `speed` + 1 jiffies, slides voice `voice`'s
    /// volume by `amount` towards `target`.
    Envelope {
        amount: u8,
        speed: u8,
        voice: u8,
        target: u8,
    },
    /// `$F8 <GsPt>`: saves the program counter, then jumps as `$F2`.
    GoSub { pattern: u8, step: u16 },
    /// `$F9 <RoPt>`: resumes at the program counter saved by `$F8`.
    Return,
    /// `$FA <Fade>`: every `speed` jiffies, slides the master volume by 1
    /// towards `target`.
    Fade { speed: u8, target: u8 },
    /// `$FB <PPat>`: jumps track `track` to pattern `pattern` with
    /// `transpose`, and continues. Whether the jump is immediate or takes
    /// effect on the next entry into the play routine depends on the two
    /// track numbers -- `docs/opcodes.md` §2.
    PlayPattern {
        pattern: u8,
        track: u8,
        transpose: i8,
    },
    /// `$FC <Port>`: every `speed` jiffies, multiplies voice `voice`'s
    /// period by `(256 + rate) / 256`.
    Portamento { speed: u8, voice: u8, rate: u8 },
    /// `$FD <Lock>`: locks `channel` (already masked to `aa & 3`) against
    /// other notes for `ticks` ticks.
    Lock { channel: u8, ticks: u16 },
    /// `$FE <StCu>`: [S1] states the same effect as `$F4` but explicitly
    /// does not know what distinguishes the two -- see the Unresolved
    /// section of `docs/opcodes.md`. Kept as its own variant so a player
    /// that later learns the difference has somewhere to put it; this crate
    /// halts on it exactly as on `$F4`.
    StopCustom,
    /// `$FF <NOP!>`: does nothing; the next entry is fetched immediately.
    Nop,
}

fn decode_pattern_command(bytes: [u8; 4]) -> PatternCommand {
    let [opcode, aa, bb, cc] = bytes;
    let word = u16::from_be_bytes([bb, cc]);
    match opcode {
        0xF0 => PatternCommand::End,
        0xF1 => PatternCommand::Loop {
            times: aa,
            target: word,
        },
        0xF2 => PatternCommand::Jump {
            pattern: aa,
            step: word,
        },
        0xF3 => PatternCommand::Wait { jiffies: aa },
        0xF4 => PatternCommand::Stop,
        0xF5 => PatternCommand::KeyUp { voice: bb & 0x0F },
        0xF6 => PatternCommand::Vibrato {
            speed: aa,
            voice: bb & 0x0F,
            depth: cc,
        },
        0xF7 => PatternCommand::Envelope {
            amount: aa,
            speed: bb >> 4,
            voice: bb & 0x0F,
            target: cc,
        },
        0xF8 => PatternCommand::GoSub {
            pattern: aa,
            step: word,
        },
        0xF9 => PatternCommand::Return,
        0xFA => PatternCommand::Fade {
            speed: aa,
            target: cc,
        },
        0xFB => PatternCommand::PlayPattern {
            pattern: aa,
            track: bb & 0x0F,
            transpose: cc as i8,
        },
        0xFC => PatternCommand::Portamento {
            speed: aa,
            voice: bb & 0x0F,
            rate: cc,
        },
        0xFD => PatternCommand::Lock {
            channel: aa & 3,
            ticks: word,
        },
        0xFE => PatternCommand::StopCustom,
        _ => PatternCommand::Nop,
    }
}

fn decode_pattern_entry(bytes: [u8; 4]) -> PatternEntry {
    let [aa, bb, cv, dd] = bytes;
    if aa >= 0xF0 {
        return PatternEntry::Command(decode_pattern_command(bytes));
    }
    PatternEntry::Note {
        note: aa & 0x3F,
        macro_number: bb,
        volume: cv >> 4,
        voice: cv & 0x0F,
        timing: match aa & 0xC0 {
            0xC0 => NoteTiming::Portamento(dd),
            0x80 => NoteTiming::Wait(dd),
            _ => NoteTiming::Detune(dd as i8),
        },
    }
}

/// Entries executed in a single jiffy before the runner gives up. Only
/// reachable through data that loops with no wait inside the loop.
const MAX_PATTERN_ENTRIES_PER_JIFFY: usize = 1024;

/// The pattern runner: one track's pattern program counter, its pending
/// wait, its `$F1` repeat counter and its `$F8` return address.
///
/// [`PatternRunner::advance`] is an explicit, externally-triggered step of
/// one jiffy, for the same reason [`Sequencer::advance`] is: the runner owns
/// *what* runs and *how long it waits*, and the caller owns the clock.
///
/// Every target in `$F1`/`$F2`/`$F8` is a **longword step index relative to
/// the pattern it names**, never a byte offset and never absolute -- the two
/// offset spaces are separate (`ROADMAP.md`, "Finding from 1.3", verified
/// across all 229 such commands in the corpus).
///
/// Executing what the entries mean -- triggering macros, vibrato, envelopes
/// -- is the macro interpreter's job (step 4.4). This type only decodes,
/// sequences and times them.
#[derive(Debug)]
pub struct PatternRunner<'a> {
    module: &'a Module<'a>,
    pattern: u8,
    step: u16,
    /// Jiffies still to pass before the next entry is fetched.
    wait: u16,
    /// Passes left for the `$F1` currently in progress. One counter for the
    /// whole pattern, as the single program counter implies.
    repeat: Option<u8>,
    /// Program counter saved by `$F8`, restored by `$F9`.
    saved: Option<(u8, u16)>,
    /// The terminal command that stopped this runner, if any.
    halted: Option<PatternCommand>,
}

impl<'a> PatternRunner<'a> {
    /// A runner positioned at longword step 0 of pattern `pattern`.
    pub fn new(module: &'a Module<'a>, pattern: u8) -> Result<Self, AccessError> {
        module.pattern(pattern)?;
        Ok(Self {
            module,
            pattern,
            step: 0,
            wait: 0,
            repeat: None,
            saved: None,
            halted: None,
        })
    }

    /// The pattern being executed; `$F2`/`$F8`/`$F9` can change it.
    pub fn pattern(&self) -> u8 {
        self.pattern
    }

    /// The longword step about to be executed.
    pub fn step(&self) -> u16 {
        self.step
    }

    /// The terminal command (`$F0`, `$F4` or `$FE`) that ended this pattern,
    /// or `None` while it is still running.
    pub fn halted(&self) -> Option<PatternCommand> {
        self.halted
    }

    /// Advances one jiffy, calling `emit` for every entry executed during
    /// it -- none while a wait runs down, one for `$F3`, and as many as the
    /// data chains for immediate-fetch notes and flow-control commands.
    pub fn advance(&mut self, mut emit: impl FnMut(PatternEntry)) -> Result<(), AccessError> {
        if self.halted.is_some() {
            return Ok(());
        }
        if self.wait > 0 {
            self.wait -= 1;
            return Ok(());
        }
        // ponytail: bounded rather than unbounded, so pattern data that
        // loops without ever waiting cannot hang the player. The program
        // counter is left where it is; the next jiffy resumes from there.
        for _ in 0..MAX_PATTERN_ENTRIES_PER_JIFFY {
            let entry = decode_pattern_entry(self.fetch()?);
            emit(entry);
            if !self.apply(entry) {
                break;
            }
        }
        Ok(())
    }

    /// The four bytes at the current program counter.
    fn fetch(&self) -> Result<[u8; 4], AccessError> {
        let data = self.module.pattern(self.pattern)?;
        let start = self.step as usize * 4;
        let bytes = data.get(start..start + 4).ok_or(AccessError::OutOfRange)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Applies `entry`'s flow and timing effect. Returns whether the runner
    /// should keep fetching entries within this same jiffy.
    fn apply(&mut self, entry: PatternEntry) -> bool {
        let command = match entry {
            PatternEntry::Note { timing, .. } => {
                self.step += 1;
                if let NoteTiming::Wait(wait) = timing {
                    // The note occupies `wait` + 1 jiffies, of which this
                    // one is the first.
                    self.wait = u16::from(wait);
                    return false;
                }
                return true;
            }
            PatternEntry::Command(command) => command,
        };
        match command {
            PatternCommand::End | PatternCommand::Stop | PatternCommand::StopCustom => {
                self.halted = Some(command);
                return false;
            }
            PatternCommand::Wait { jiffies } => {
                self.step += 1;
                self.wait = u16::from(jiffies);
                return false;
            }
            PatternCommand::Loop { times, target } => {
                let left = *self.repeat.get_or_insert(times);
                if times == 0 {
                    self.step = target;
                } else if left == 0 {
                    self.repeat = None;
                    self.step += 1;
                } else {
                    self.repeat = Some(left - 1);
                    self.step = target;
                }
            }
            PatternCommand::Jump { pattern, step } => {
                self.pattern = pattern;
                self.step = step;
            }
            PatternCommand::GoSub { pattern, step } => {
                self.saved = Some((self.pattern, self.step + 1));
                self.pattern = pattern;
                self.step = step;
            }
            PatternCommand::Return => match self.saved.take() {
                Some((pattern, step)) => {
                    self.pattern = pattern;
                    self.step = step;
                }
                // Nothing was saved; treat it as a no-op rather than guess.
                None => self.step += 1,
            },
            // Recognized, timed, and handed to the caller -- their effects
            // belong to voices and to the master volume, which this type
            // does not own (step 4.4).
            PatternCommand::KeyUp { .. }
            | PatternCommand::Vibrato { .. }
            | PatternCommand::Envelope { .. }
            | PatternCommand::Fade { .. }
            | PatternCommand::PlayPattern { .. }
            | PatternCommand::Portamento { .. }
            | PatternCommand::Lock { .. }
            | PatternCommand::Nop => self.step += 1,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{AccessError, Module};

    /// Runs `total` samples in chunks of `chunk`, collecting the cumulative
    /// sample offset of every tick that fires.
    fn tick_offsets(clock: &mut TickClock, sample_rate: u32, total: u32, chunk: u32) -> Vec<u32> {
        let mut offsets = Vec::new();
        let mut base = 0;
        while base < total {
            let n = chunk.min(total - base);
            let mut pos = 0;
            clock.advance(sample_rate, n, |tick_due, frames| {
                if tick_due {
                    offsets.push(base + pos);
                }
                pos += frames;
            });
            base += n;
        }
        offsets
    }

    #[test]
    fn divider_path_fraction() {
        assert_eq!(tick_fraction(0, 44100), (44100, 50)); // 882 exactly
        assert_eq!(tick_fraction(0, 48000), (48000, 50)); // 960 exactly
        assert_eq!(tick_fraction(2, 48000), (48000 * 3, 50)); // 50/3 Hz
        assert_eq!(tick_fraction(15, 48000), (48000 * 16, 50));
    }

    #[test]
    fn cia_path_fraction() {
        // v = 125 -> 125*24/60 = 50 Hz; v = 140 -> 56 Hz.
        assert_eq!(tick_fraction(125, 48000), (48000 * 60, 125 * 24));
        assert_eq!(tick_fraction(140, 44100), (44100 * 60, 140 * 24));
        // 16 is the first value on the CIA path.
        assert_eq!(tick_fraction(16, 48000), (48000 * 60, 16 * 24));
    }

    #[test]
    fn integer_tick_lengths_are_exact() {
        let mut clock = TickClock::new(0);
        assert_eq!(clock.samples_until_next_tick(48000), 0); // first tick due
        let offsets = tick_offsets(&mut clock, 48000, 48000, 48000);
        assert_eq!(offsets.len(), 50);
        assert!(
            offsets
                .iter()
                .enumerate()
                .all(|(i, &o)| o as usize == i * 960)
        );
        assert_eq!(clock.acc, 0);
    }

    #[test]
    fn non_integer_tick_length_alternates_without_drift() {
        // 44100 Hz at BPM 140 = 787.5 samples per tick.
        let mut clock = TickClock::new(140);
        let offsets = tick_offsets(&mut clock, 44100, 44100 * 4, 44100 * 4);
        assert_eq!(&offsets[..5], &[0, 787, 1575, 2362, 3150]);
        // 56 ticks per second, each landing on the exact rounded-down index.
        assert_eq!(offsets.len(), 56 * 4);
        assert!(
            offsets
                .iter()
                .enumerate()
                .all(|(i, &o)| u64::from(o) == (i as u64 * 44100 * 60) / (140 * 24))
        );
    }

    #[test]
    fn divisor_zero_and_bpm_125_schedule_identically() {
        let mut divider = TickClock::new(0);
        let mut bpm = TickClock::new(125);
        assert_eq!(
            tick_offsets(&mut divider, 48000, 48000, 480),
            tick_offsets(&mut bpm, 48000, 48000, 480)
        );
        assert_eq!(divider.acc, 0);
        assert_eq!(bpm.acc, 0);
        assert_eq!(
            divider.samples_until_next_tick(48000),
            bpm.samples_until_next_tick(48000)
        );
    }

    #[test]
    fn block_size_independence() {
        // The step's check: 1 second at 48000 Hz as one call and as 480
        // hundred-sample calls must schedule identically. Repeated across
        // tempo values including non-integer tick lengths.
        for &(tempo, sample_rate) in &[
            (0u16, 48000u32),
            (2, 48000),
            (125, 48000),
            (140, 44100),
            (140, 48000),
            (33, 44100),
        ] {
            let total = sample_rate;
            let mut whole = TickClock::new(tempo);
            let one_call = tick_offsets(&mut whole, sample_rate, total, total);
            for &chunk in &[1u32, 7, 100, 512, 4801] {
                let mut chunked = TickClock::new(tempo);
                let many = tick_offsets(&mut chunked, sample_rate, total, chunk);
                assert_eq!(
                    one_call, many,
                    "tempo {tempo} @ {sample_rate} chunk {chunk}"
                );
                assert_eq!(whole, chunked, "state differs: tempo {tempo} chunk {chunk}");
            }
        }
    }

    #[test]
    fn spans_cover_every_sample_exactly_once() {
        let mut clock = TickClock::new(140);
        let mut covered = 0;
        clock.advance(44100, 5000, |_, frames| covered += frames);
        assert_eq!(covered, 5000);
        clock.advance(44100, 0, |_, _| panic!("no spans for an empty request"));
    }

    #[test]
    fn tempo_change_keeps_the_phase() {
        let mut clock = TickClock::new(140);
        clock.advance(44100, 1000, |_, _| {});
        let acc = clock.acc;
        clock.set_tempo(125);
        assert_eq!(clock.acc, acc);
        assert_eq!(clock.tempo(), 125);
    }

    // -- Trackstep line decoding (step 4.2) --
    // `docs/format.md` §5 and `docs/opcodes.md` §1.

    #[test]
    fn decodes_plain_pattern_word() {
        // $54E8: pattern $54 (84), transpose two's-complement $E8 = -24.
        // Real bytes from `mdat.turrican intro` trackstep line 76 word 0.
        assert_eq!(
            decode_track_word(0x54E8),
            TrackSlot::Pattern {
                number: 0x54,
                transpose: -24
            }
        );
        assert_eq!(
            decode_track_word(0x6B00),
            TrackSlot::Pattern {
                number: 0x6B,
                transpose: 0
            }
        );
        assert_eq!(
            decode_track_word(0x0001),
            TrackSlot::Pattern {
                number: 0x00,
                transpose: 1
            }
        );
    }

    #[test]
    fn decodes_undocumented_high_bit_as_masked_pattern_number() {
        // $FA01: hi byte $FA falls outside every documented case ($00-$7F
        // pattern, $80 hold, $FE stop voice, $FF stop channel) --
        // `docs/format.md` §5 gives no meaning to $81-$FD. Real word from
        // `mdat.r-type` song 0 trackstep line 79, the song's declared
        // (inclusive) end line, which real playback does reach every loop.
        // Only 7 bits are ever needed for a pattern number (max 128
        // patterns), so this crate reads $80 as a sentinel bit and masks it
        // off for every other value rather than erroring out the whole
        // render over one stray bit -- the same tolerance already applied
        // to the voice nibble in `Player::voice_of`.
        assert_eq!(
            decode_track_word(0xFA01),
            TrackSlot::Pattern {
                number: 0x7A,
                transpose: 1
            }
        );
    }

    #[test]
    fn decodes_hold_with_transpose() {
        // $8008: hold last position, transpose +8. Real word from
        // `mdat.turrican 3 level 1` trackstep line 6.
        assert_eq!(decode_track_word(0x8008), TrackSlot::Hold { transpose: 8 });
        // $801F: hold, transpose $1F = +31. From `mdat.turrican outside`.
        assert_eq!(decode_track_word(0x801F), TrackSlot::Hold { transpose: 31 });
    }

    #[test]
    fn decodes_stop_channel_and_stop_voice() {
        assert_eq!(decode_track_word(0xFF00), TrackSlot::StopChannel);
        assert_eq!(decode_track_word(0xFF7F), TrackSlot::StopChannel);
        assert_eq!(decode_track_word(0xFE01), TrackSlot::StopVoice { voice: 1 });
        assert_eq!(decode_track_word(0xFE00), TrackSlot::StopVoice { voice: 0 });
    }

    #[test]
    fn decodes_non_effe_line_as_eight_track_slots() {
        // Real bytes: `mdat.turrican intro` trackstep line 76.
        let bytes: [u8; 16] = [
            0x54, 0xE8, 0x6B, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
            0xFF, 0x00,
        ];
        let TrackstepLine::Tracks(slots) = decode_line(&bytes) else {
            panic!("expected a track-data line");
        };
        assert_eq!(
            slots[0],
            TrackSlot::Pattern {
                number: 0x54,
                transpose: -24
            }
        );
        assert_eq!(
            slots[1],
            TrackSlot::Pattern {
                number: 0x6B,
                transpose: 0
            }
        );
        for slot in &slots[2..] {
            assert_eq!(*slot, TrackSlot::StopChannel);
        }
    }

    #[test]
    fn decodes_effe_stop() {
        let bytes: [u8; 16] = [0xEF, 0xFE, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            decode_line(&bytes),
            TrackstepLine::Command(LineCommand::Stop)
        );
    }

    #[test]
    fn decodes_effe_play_section() {
        // Real bytes: `mdat.turrican intro` trackstep line 129 (song 0's end).
        let bytes: [u8; 16] = [
            0xEF, 0xFE, 0x00, 0x01, 0x00, 0x4D, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
            0xFF, 0x00,
        ];
        assert_eq!(
            decode_line(&bytes),
            TrackstepLine::Command(LineCommand::PlaySection {
                position: 77,
                times: 0xFF00
            })
        );
    }

    #[test]
    fn decodes_effe_set_tempo() {
        let bytes: [u8; 16] = [
            0xEF, 0xFE, 0x00, 0x02, 0x00, 0x05, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(
            decode_line(&bytes),
            TrackstepLine::Command(LineCommand::SetTempo {
                divisor: 5,
                cia_bpm: 0xFFFF
            })
        );
    }

    #[test]
    fn decodes_effe_master_vol_slide_a_and_b() {
        // Real bytes: `mdat.turrican intro` trackstep line 75 ($EFFE0004).
        let bytes: [u8; 16] = [
            0xEF, 0xFE, 0x00, 0x04, 0x00, 0x00, 0x00, 0x40, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
            0xFF, 0x00,
        ];
        assert_eq!(
            decode_line(&bytes),
            TrackstepLine::Command(LineCommand::MasterVolSlideB {
                divisor: 0,
                target: 0x40
            })
        );

        let mut a = bytes;
        a[3] = 0x03;
        assert_eq!(
            decode_line(&a),
            TrackstepLine::Command(LineCommand::MasterVolSlideA {
                divisor: 0,
                target: 0x40
            })
        );
    }

    // -- Sequencer: the trackstep runner (step 4.2) --

    /// Builds a minimal fixed-layout `mdat` (trackstep at `$800`, song 0's
    /// start/end/tempo in the header table) holding exactly `lines`.
    fn fixed_layout_module(song_end: u16, tempo: u16, lines: &[[u8; 16]]) -> Vec<u8> {
        let mut mdat = vec![0u8; 0x800 + lines.len() * 16];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x140..0x142].copy_from_slice(&song_end.to_be_bytes());
        mdat[0x180..0x182].copy_from_slice(&tempo.to_be_bytes());
        for (i, line) in lines.iter().enumerate() {
            let o = 0x800 + i * 16;
            mdat[o..o + 16].copy_from_slice(line);
        }
        mdat
    }

    const STOP_LINE: [u8; 16] = [
        0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF,
        0x00,
    ];

    fn pattern_line(word0: u16) -> [u8; 16] {
        let mut line = STOP_LINE;
        line[0..2].copy_from_slice(&word0.to_be_bytes());
        line
    }

    fn effe_line(sub: u16, param_a: u16, param_b: u16) -> [u8; 16] {
        let mut line = [0u8; 16];
        line[0..2].copy_from_slice(&EFFE.to_be_bytes());
        line[2..4].copy_from_slice(&sub.to_be_bytes());
        line[4..6].copy_from_slice(&param_a.to_be_bytes());
        line[6..8].copy_from_slice(&param_b.to_be_bytes());
        line
    }

    #[test]
    fn advance_treats_an_unreadable_line_as_an_implicit_stop() {
        // The header's 32-slot song table doesn't guarantee every slot is
        // actually used -- an unused/placeholder slot can point past the
        // module's real trackstep table (observed in a real corpus file:
        // `apidya (title)` song 31 has song_start = song_end = 511, one
        // line past its 511-line table). That must behave like an
        // `$EFFE 0000 Stop` line, not surface `AccessError`.
        let mut mdat = fixed_layout_module(5, 0, &[STOP_LINE]);
        mdat[0x100..0x102].copy_from_slice(&5u16.to_be_bytes()); // song_start = 5: no such line
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        let line = seq.advance().expect("unreadable line must not error");
        assert_eq!(line, TrackstepLine::Command(LineCommand::Stop));
        assert!(seq.is_stopped());
    }

    #[test]
    fn new_rejects_out_of_range_song() {
        let mdat = fixed_layout_module(0, 0, &[STOP_LINE]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        assert_eq!(
            Sequencer::new(&module, 32).unwrap_err(),
            AccessError::OutOfRange
        );
    }

    #[test]
    fn new_starts_at_song_start_with_song_tempo() {
        let mdat = fixed_layout_module(0, 140, &[STOP_LINE]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let seq = Sequencer::new(&module, 0).expect("song 0 in range");
        assert_eq!(seq.current_line(), 0); // song_start defaults to 0
        assert_eq!(seq.tempo(), 140);
        assert!(!seq.is_stopped());
    }

    #[test]
    fn advance_loads_track_slots_from_the_line() {
        let mdat = fixed_layout_module(0, 0, &[pattern_line(0x54E8)]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        seq.advance().expect("line in range");
        assert_eq!(
            seq.track(0),
            TrackSlot::Pattern {
                number: 0x54,
                transpose: -24
            }
        );
        for t in 1..8 {
            assert_eq!(seq.track(t), TrackSlot::StopChannel);
        }
    }

    #[test]
    fn advance_hold_keeps_pattern_but_updates_transpose() {
        let lines = [pattern_line(0x5400), pattern_line(0x8008)];
        let mdat = fixed_layout_module(1, 0, &lines);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        seq.advance().expect("line 0 in range"); // loads pattern $54, transpose 0
        seq.advance().expect("line 1 in range"); // $80 hold, transpose +8
        assert_eq!(
            seq.track(0),
            TrackSlot::Pattern {
                number: 0x54,
                transpose: 8
            }
        );
    }

    #[test]
    fn advance_wraps_song_end_to_song_start() {
        let lines = [pattern_line(0x0100), pattern_line(0x0200)];
        let mdat = fixed_layout_module(1, 0, &lines); // song_end = line 1
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        seq.advance().unwrap(); // line 0 -> line 1
        assert_eq!(seq.current_line(), 1);
        seq.advance().unwrap(); // line 1 (== song_end) -> wraps to song_start (0)
        assert_eq!(seq.current_line(), 0);
    }

    #[test]
    fn advance_set_tempo_prefers_cia_bpm_unless_sentinel() {
        let lines = [effe_line(0x0002, 5, 0xFFFF), effe_line(0x0002, 5, 140)];
        let mdat = fixed_layout_module(1, 0, &lines);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        seq.advance().unwrap(); // cia_bpm = $FFFF -> "no change" -> use divisor
        assert_eq!(seq.tempo(), 5);
        seq.advance().unwrap(); // cia_bpm = 140 -> active
        assert_eq!(seq.tempo(), 140);
    }

    #[test]
    fn advance_stop_halts_and_further_advances_are_a_no_op() {
        let mdat = fixed_layout_module(0, 0, &[effe_line(0x0000, 0, 0)]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        seq.advance().unwrap();
        assert!(seq.is_stopped());
        let line_before = seq.current_line();
        seq.advance().unwrap();
        assert_eq!(seq.current_line(), line_before);
        assert!(seq.is_stopped());
    }

    #[test]
    fn advance_play_section_repeats_then_falls_through_to_song_loop() {
        // line 0: plain track data. line 1: PlaySection(position=0, times=2).
        // song_end = 1, so once the section is exhausted the fall-through
        // advance past line 1 wraps back to song_start (0) too -- the two
        // looping mechanisms coincide here by construction.
        let lines = [pattern_line(0x0100), effe_line(0x0001, 0, 2)];
        let mdat = fixed_layout_module(1, 0, &lines);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");

        let mut trace = Vec::new();
        for _ in 0..7 {
            seq.advance().unwrap();
            trace.push(seq.current_line());
        }
        // 2 extra repeats of [0,1] before falling through and re-wrapping.
        assert_eq!(trace, vec![1, 0, 1, 0, 1, 0, 1]);
    }

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(path).ok()
    }

    /// Every `mdat.*` file present in `testdata/`, or an empty list if the
    /// corpus has not been fetched.
    fn corpus_mdats() -> Vec<std::path::PathBuf> {
        let dir = format!("{}/../testdata", env!("CARGO_MANIFEST_DIR"));
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("mdat."))
            })
            .collect();
        paths.sort();
        paths
    }

    /// The step's own check: trace the first 200 advances of a real corpus
    /// file's song against `docs/format.md` §5 / `docs/opcodes.md` §1.
    /// `mdat.turrican intro` song 0 runs lines 75-129; line 129 is a
    /// `$EFFE 0001 PlaySection(position=77, times=$FF00)`, so past the
    /// first pass through 76-129 the trace repeats the 53-line cycle
    /// 77-129 -- confirmed here by direct byte inspection, independent of
    /// this implementation.
    #[test]
    fn traces_first_200_advances_of_a_real_song() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");
        assert_eq!(module.song_start(0), 75);
        assert_eq!(module.song_end(0), 129);
        assert_eq!(module.tempo(0), 3);

        let mut seq = Sequencer::new(&module, 0).expect("song 0 in range");
        assert_eq!(seq.current_line(), 75);

        let mut trace = Vec::with_capacity(200);
        for _ in 0..200 {
            seq.advance().expect("trackstep line in range");
            trace.push(seq.current_line());
        }

        // First pass runs the lines in order up to song_end.
        assert_eq!(&trace[0..5], &[76, 77, 78, 79, 80]);
        // Line 129 (song_end) is the PlaySection command: it fires instead
        // of a plain +1, jumping back to position 77.
        assert_eq!(trace[53], 129);
        assert_eq!(trace[54], 77);
        // The 53-line cycle (77-129) repeats identically on each pass.
        assert_eq!(trace[106], 129);
        assert_eq!(trace[107], 77);
        assert_eq!(trace[199], 116);
        // $FF00 (65280) repeats is nowhere near exhausted in 200 steps.
        assert!(!seq.is_stopped());
        // No $EFFE 0002 SetTempo in these 200 lines -- still the header
        // slot's tempo.
        assert_eq!(seq.tempo(), 3);
    }

    // -- Pattern longword decoding (step 4.3) --
    // `docs/format.md` §6 and `docs/opcodes.md` §2.

    #[test]
    fn decodes_note_with_wait() {
        // `98 2F 50 07`: the worked example in `docs/format.md` §6, the first
        // longword of pattern $00 in `mdat.turrican 2 level 1-desert`.
        // Top bits 10 -> note + wait; note $18, macro $2F, c=5, v=0, wait 7.
        assert_eq!(
            decode_pattern_entry([0x98, 0x2F, 0x50, 0x07]),
            PatternEntry::Note {
                note: 0x18,
                macro_number: 0x2F,
                volume: 5,
                voice: 0,
                timing: NoteTiming::Wait(7),
            }
        );
        // `A0 18 C3 03`: `mdat.turrican intro` pattern $00 step 0.
        assert_eq!(
            decode_pattern_entry([0xA0, 0x18, 0xC3, 0x03]),
            PatternEntry::Note {
                note: 0x20,
                macro_number: 0x18,
                volume: 0xC,
                voice: 3,
                timing: NoteTiming::Wait(3),
            }
        );
    }

    #[test]
    fn decodes_note_with_detune() {
        // `1F 30 52 00`: second worked example in `docs/format.md` §6
        // (`mdat.turrican 2 level 1-desert`). Top bits 00 -> detune, and the
        // next entry is fetched in the same jiffy.
        assert_eq!(
            decode_pattern_entry([0x1F, 0x30, 0x52, 0x00]),
            PatternEntry::Note {
                note: 0x1F,
                macro_number: 0x30,
                volume: 5,
                voice: 2,
                timing: NoteTiming::Detune(0),
            }
        );
        // `22 12 83 00`: `mdat.turrican intro` pattern $08 step 1.
        assert_eq!(
            decode_pattern_entry([0x22, 0x12, 0x83, 0x00]),
            PatternEntry::Note {
                note: 0x22,
                macro_number: 0x12,
                volume: 8,
                voice: 3,
                timing: NoteTiming::Detune(0),
            }
        );
        // Top bits 01 is the same class as 00, and only the low 6 bits of
        // the first byte select the note ($7F -> note $3F). The detune byte
        // is signed.
        assert_eq!(
            decode_pattern_entry([0x7F, 0x01, 0x00, 0xFF]),
            PatternEntry::Note {
                note: 0x3F,
                macro_number: 0x01,
                volume: 0,
                voice: 0,
                timing: NoteTiming::Detune(-1),
            }
        );
    }

    #[test]
    fn decodes_portamento_note() {
        // `D6 01 C1 03`: `mdat.turrican intro` pattern $14 step 2. Top bits
        // 11 with a first byte below $F0 -> portamento to note $16, rate $03.
        assert_eq!(
            decode_pattern_entry([0xD6, 0x01, 0xC1, 0x03]),
            PatternEntry::Note {
                note: 0x16,
                macro_number: 0x01,
                volume: 0xC,
                voice: 1,
                timing: NoteTiming::Portamento(3),
            }
        );
        // `EF` is the last portamento first-byte; `$F0` upwards are commands.
        assert!(matches!(
            decode_pattern_entry([0xEF, 0x00, 0x00, 0x01]),
            PatternEntry::Note {
                note: 0x2F,
                timing: NoteTiming::Portamento(1),
                ..
            }
        ));
    }

    #[test]
    fn decodes_pattern_commands() {
        use PatternCommand::*;
        // One row per opcode of `docs/opcodes.md` §2, in table order.
        let cases: [([u8; 4], PatternCommand); 16] = [
            ([0xF0, 0x00, 0x00, 0x00], End),
            // `F1 00 00 01`: `mdat.turrican intro` pattern $08 -- infinite
            // repeat of the block starting at longword step 1.
            (
                [0xF1, 0x00, 0x00, 0x01],
                Loop {
                    times: 0,
                    target: 1,
                },
            ),
            // `F2 2D 00 01`: `mdat.turrican intro` pattern $5C -- pattern
            // $2D at longword step 1.
            (
                [0xF2, 0x2D, 0x00, 0x01],
                Jump {
                    pattern: 0x2D,
                    step: 1,
                },
            ),
            ([0xF3, 0x1F, 0x00, 0x00], Wait { jiffies: 0x1F }),
            ([0xF4, 0x00, 0x00, 0x00], Stop),
            // `F5 08 82 00`: `mdat.turrican intro` pattern $48.
            ([0xF5, 0x08, 0x82, 0x00], KeyUp { voice: 2 }),
            // `F6 05 02 03`: `mdat.turrican outside` pattern $27.
            (
                [0xF6, 0x05, 0x02, 0x03],
                Vibrato {
                    speed: 5,
                    voice: 2,
                    depth: 3,
                },
            ),
            (
                [0xF7, 0x02, 0x31, 0x40],
                Envelope {
                    amount: 2,
                    speed: 3,
                    voice: 1,
                    target: 0x40,
                },
            ),
            (
                [0xF8, 0x10, 0x00, 0x05],
                GoSub {
                    pattern: 0x10,
                    step: 5,
                },
            ),
            ([0xF9, 0x00, 0x00, 0x00], Return),
            (
                [0xFA, 0x04, 0x00, 0x20],
                Fade {
                    speed: 4,
                    target: 0x20,
                },
            ),
            // `FB 65 04 00`: `mdat.turrican intro` pattern $66.
            (
                [0xFB, 0x65, 0x04, 0x00],
                PlayPattern {
                    pattern: 0x65,
                    track: 4,
                    transpose: 0,
                },
            ),
            // `FC 36 02 30`: `mdat.turrican 3 level 1` pattern $46.
            (
                [0xFC, 0x36, 0x02, 0x30],
                Portamento {
                    speed: 0x36,
                    voice: 2,
                    rate: 0x30,
                },
            ),
            // `FD 00 FF FF`: `mdat.apidya (title)` pattern $29. `aa` is
            // masked to a channel by `aa & 3`.
            (
                [0xFD, 0x06, 0xFF, 0xFF],
                Lock {
                    channel: 2,
                    ticks: 0xFFFF,
                },
            ),
            ([0xFE, 0x00, 0x00, 0x00], StopCustom),
            ([0xFF, 0x00, 0x00, 0x00], Nop),
        ];
        for (bytes, expected) in cases {
            assert_eq!(
                decode_pattern_entry(bytes),
                PatternEntry::Command(expected),
                "{bytes:02X?}"
            );
        }
        // `$FE` is its own variant, never folded into `$F4`: [S1] documents
        // the same base effect but explicitly does not know what
        // distinguishes them (`docs/opcodes.md`, Unresolved).
        assert_ne!(
            decode_pattern_entry([0xFE, 0, 0, 0]),
            decode_pattern_entry([0xF4, 0, 0, 0])
        );
    }

    // -- PatternRunner (step 4.3) --

    /// Builds a fixed-layout `mdat` whose pattern-pointer table points at
    /// each of `patterns` in turn.
    fn pattern_module(patterns: &[&[[u8; 4]]]) -> Vec<u8> {
        let mut mdat = vec![0u8; 0x900];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let mut offset = 0x900u32;
        for (i, entries) in patterns.iter().enumerate() {
            let slot = 0x400 + i * 4;
            mdat[slot..slot + 4].copy_from_slice(&offset.to_be_bytes());
            offset += (entries.len() * 4) as u32;
        }
        for entries in patterns {
            for entry in *entries {
                mdat.extend_from_slice(entry);
            }
        }
        mdat
    }

    /// Runs `jiffies` jiffies of pattern 0, collecting `(jiffy, entry)` for
    /// every entry executed.
    fn run_pattern(module: &Module, jiffies: u32) -> Vec<(u32, PatternEntry)> {
        let mut runner = PatternRunner::new(module, 0).expect("pattern 0 in range");
        let mut log = Vec::new();
        for jiffy in 0..jiffies {
            runner
                .advance(|entry| log.push((jiffy, entry)))
                .expect("pattern stays in range");
        }
        log
    }

    #[test]
    fn out_of_range_pattern_is_rejected() {
        let mdat = pattern_module(&[&[[0xF0, 0, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        assert_eq!(
            PatternRunner::new(&module, 128).unwrap_err(),
            AccessError::OutOfRange
        );
    }

    #[test]
    fn note_wait_holds_the_program_counter_for_wait_plus_one_jiffies() {
        // A note with wait 2 occupies 3 jiffies, then `$F3 00` occupies one
        // more, then the pattern ends.
        let mdat = pattern_module(&[&[
            [0x80, 0x01, 0x00, 0x02],
            [0xF3, 0x00, 0x00, 0x00],
            [0xF0, 0x00, 0x00, 0x00],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let log = run_pattern(&module, 6);
        let jiffies: Vec<u32> = log.iter().map(|(j, _)| *j).collect();
        assert_eq!(jiffies, vec![0, 3, 4]);
        assert!(matches!(
            log[2].1,
            PatternEntry::Command(PatternCommand::End)
        ));
    }

    #[test]
    fn detune_and_portamento_notes_are_fetched_in_the_same_jiffy() {
        // `aa` < $80 and `aa` > $BF both carry no wait: everything up to the
        // wait command runs inside jiffy 0.
        let mdat = pattern_module(&[&[
            [0x10, 0x01, 0x00, 0x00],
            [0xC5, 0x01, 0x00, 0x04],
            [0xFF, 0x00, 0x00, 0x00],
            [0xF3, 0x00, 0x00, 0x00],
            [0xF0, 0x00, 0x00, 0x00],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let log = run_pattern(&module, 3);
        assert_eq!(
            log.iter().map(|(j, _)| *j).collect::<Vec<_>>(),
            vec![0, 0, 0, 0, 1]
        );
    }

    #[test]
    fn loop_repeats_the_block_then_falls_through() {
        // `$F1 02 0000`: two extra passes over step 0, matching the reading
        // `Sequencer` applies to `$EFFE 0001 PlaySection`'s repeat count.
        let mdat = pattern_module(&[&[
            [0xF3, 0x00, 0x00, 0x00],
            [0xF1, 0x02, 0x00, 0x00],
            [0xF0, 0x00, 0x00, 0x00],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let log = run_pattern(&module, 10);
        let waits = log
            .iter()
            .filter(|(_, e)| matches!(e, PatternEntry::Command(PatternCommand::Wait { .. })))
            .count();
        assert_eq!(waits, 3);
        assert!(matches!(
            log.last().expect("entries ran").1,
            PatternEntry::Command(PatternCommand::End)
        ));
    }

    #[test]
    fn infinite_loop_never_halts_and_never_spins() {
        let mdat = pattern_module(&[&[
            [0xF3, 0x00, 0x00, 0x00],
            [0xF1, 0x00, 0x00, 0x00],
            [0xF0, 0x00, 0x00, 0x00],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let log = run_pattern(&module, 100);
        // Jiffy 0 runs the `$F3` alone; every later jiffy runs the `$F1`,
        // wraps to step 0 and runs the `$F3` again. The `$F0` is dead code.
        assert_eq!(log.len(), 1 + 99 * 2);
        let mut runner = PatternRunner::new(&module, 0).expect("pattern 0 in range");
        for _ in 0..100 {
            runner.advance(|_| {}).expect("in range");
        }
        assert_eq!(runner.halted(), None);
    }

    /// A `$F1` whose block contains no wait would spin forever; the runner
    /// bounds the entries it executes per jiffy instead of hanging.
    #[test]
    fn a_waitless_loop_is_bounded_not_hung() {
        let mdat = pattern_module(&[&[[0xFF, 0x00, 0x00, 0x00], [0xF1, 0x00, 0x00, 0x00]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let log = run_pattern(&module, 2);
        assert_eq!(log.len(), 2 * MAX_PATTERN_ENTRIES_PER_JIFFY);
    }

    #[test]
    fn gosub_returns_to_the_entry_after_the_call() {
        // Pattern 0: wait, `$F8` into pattern 1 step 1, wait, end.
        // Pattern 1: end (step 0, skipped), `$F9` (step 1).
        let mdat = pattern_module(&[
            &[
                [0xF3, 0x00, 0x00, 0x00],
                [0xF8, 0x01, 0x00, 0x01],
                [0xF3, 0x00, 0x00, 0x00],
                [0xF0, 0x00, 0x00, 0x00],
            ],
            &[[0xF0, 0x00, 0x00, 0x00], [0xF9, 0x00, 0x00, 0x00]],
        ]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut runner = PatternRunner::new(&module, 0).expect("pattern 0 in range");
        let mut log = Vec::new();
        for _ in 0..4 {
            runner.advance(|e| log.push(e)).expect("in range");
        }
        assert_eq!(
            log,
            vec![
                PatternEntry::Command(PatternCommand::Wait { jiffies: 0 }),
                PatternEntry::Command(PatternCommand::GoSub {
                    pattern: 1,
                    step: 1
                }),
                PatternEntry::Command(PatternCommand::Return),
                PatternEntry::Command(PatternCommand::Wait { jiffies: 0 }),
                PatternEntry::Command(PatternCommand::End),
            ]
        );
        assert_eq!(runner.pattern(), 0);
        assert_eq!(runner.halted(), Some(PatternCommand::End));
    }

    #[test]
    fn jump_switches_pattern_and_step() {
        let mdat = pattern_module(&[
            &[[0xF2, 0x01, 0x00, 0x01]],
            &[[0xF4, 0x00, 0x00, 0x00], [0xF0, 0x00, 0x00, 0x00]],
        ]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut runner = PatternRunner::new(&module, 0).expect("pattern 0 in range");
        runner.advance(|_| {}).expect("in range");
        assert_eq!(runner.pattern(), 1);
        // Target 1 is a longword step index, not a byte offset: it lands on
        // the `$F0`, not inside the `$F4`.
        assert_eq!(runner.halted(), Some(PatternCommand::End));
    }

    #[test]
    fn end_stop_and_stop_custom_halt_the_runner_distinctly() {
        for (bytes, expected) in [
            ([0xF0u8, 0, 0, 0], PatternCommand::End),
            ([0xF4, 0, 0, 0], PatternCommand::Stop),
            ([0xFE, 0, 0, 0], PatternCommand::StopCustom),
        ] {
            let mdat = pattern_module(&[&[bytes, [0xF3, 0xFF, 0x00, 0x00]]]);
            let module = Module::parse(&mdat, &[]).expect("valid header parses");
            let mut runner = PatternRunner::new(&module, 0).expect("pattern 0 in range");
            let mut count = 0;
            for _ in 0..5 {
                runner.advance(|_| count += 1).expect("in range");
            }
            assert_eq!(runner.halted(), Some(expected));
            // Halting is final: the entry after it never runs.
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn walking_past_the_end_of_mdat_is_an_error_not_a_panic() {
        // A pattern of one `$FF`, so the walk runs off the end of `mdat`.
        let mdat = pattern_module(&[&[[0xFF, 0x00, 0x00, 0x00]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut runner = PatternRunner::new(&module, 0).expect("pattern 0 in range");
        assert_eq!(runner.advance(|_| {}), Err(AccessError::OutOfRange));
    }

    // -- The step's own check: a real pattern dump is self-consistent --

    /// Walks pattern `n` linearly from step 0 to its first terminal command,
    /// returning its longword count. Follows no jumps -- this measures the
    /// pattern as stored, independently of [`PatternRunner`].
    fn pattern_length(module: &Module, n: u8) -> Option<u16> {
        let data = module.pattern(n).ok()?;
        (0..4096u16)
            .find(|step| {
                let offset = *step as usize * 4;
                data.get(offset..offset + 4).is_some_and(|b| {
                    matches!(
                        decode_pattern_entry([b[0], b[1], b[2], b[3]]),
                        PatternEntry::Command(
                            PatternCommand::End | PatternCommand::Stop | PatternCommand::StopCustom
                        )
                    )
                })
            })
            .map(|step| step + 1)
    }

    /// Pattern numbers referenced by song `song`'s trackstep lines.
    fn patterns_of_song(module: &Module, song: u8) -> Vec<u8> {
        let mut used = Vec::new();
        for line in module.song_start(song)..=module.song_end(song) {
            let Ok(bytes) = module.trackstep_line(line) else {
                continue;
            };
            if let TrackstepLine::Tracks(slots) = decode_line(bytes) {
                for slot in slots {
                    if let TrackSlot::Pattern { number, .. } = slot
                        && !used.contains(&number)
                    {
                        used.push(number);
                    }
                }
            }
        }
        used
    }

    /// Every `$F1`/`$F2`/`$F8` target in the corpus is a longword step index
    /// inside the pattern it names -- the Finding from step 1.3. A byte
    /// offset or an absolute `mdat` offset would blow past the pattern's own
    /// length almost everywhere, so this is the check that a
    /// target-space mix-up cannot survive.
    #[test]
    fn every_jump_target_lands_inside_its_pattern() {
        let mdats = corpus_mdats();
        if mdats.is_empty() {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        }
        let (mut checked, mut measured) = (0, 0);
        for path in &mdats {
            let mdat = std::fs::read(path).expect("corpus file readable");
            let module = Module::parse(&mdat, &[]).expect("corpus module parses");
            for song in 0..32 {
                for number in patterns_of_song(&module, song) {
                    // Unused song slots (and, in `mdat.r-type`, song 0's
                    // last line, which is already pattern data rather than
                    // trackstep data) name pattern numbers that are not
                    // patterns at all. A pointer with no terminal command
                    // within the walk bound is not pattern data; skip it
                    // rather than assert about garbage.
                    let Some(len) = pattern_length(&module, number) else {
                        continue;
                    };
                    measured += 1;
                    let data = module.pattern(number).expect("pattern in range");
                    for step in 0..len as usize {
                        let b = &data[step * 4..step * 4 + 4];
                        let (target_pattern, target) =
                            match decode_pattern_entry([b[0], b[1], b[2], b[3]]) {
                                PatternEntry::Command(PatternCommand::Loop { target, .. }) => {
                                    (number, target)
                                }
                                PatternEntry::Command(
                                    PatternCommand::Jump { pattern, step }
                                    | PatternCommand::GoSub { pattern, step },
                                ) => (pattern, step),
                                _ => continue,
                            };
                        let target_len = pattern_length(&module, target_pattern)
                            .expect("jump destination is a pattern");
                        assert!(
                            target < target_len,
                            "{path:?} pattern {number:#04X} step {step}: target {target} \
                             outside pattern {target_pattern:#04X} of {target_len} longwords",
                        );
                        checked += 1;
                    }
                }
            }
        }
        // The corpus is known to hold 229 such commands across all patterns;
        // the reachable-from-a-song subset checked here is smaller but must
        // not be empty, or this test proves nothing.
        assert!(measured > 500, "only {measured} patterns measured");
        assert!(checked > 100, "only {checked} targets checked");
    }

    /// The step's own check: dump one real pattern end to end and assert the
    /// properties that only hold if the classification, the wait accounting
    /// and the relative loop target are all right.
    ///
    /// `mdat.turrican intro` pattern $08 (21 longwords, hand-decoded from
    /// the file):
    ///
    /// ```text
    ///  0  F3 01 00 00   wait 1+1 = 2 jiffies
    ///  1  22 12 83 00   note $22, macro $12, vol 8, voice 3, detune 0
    ///  2  F3 03 00 00   wait 4 jiffies          |
    ///  ...              seven more note/wait pairs
    /// 19  F1 00 00 01   loop forever from step 1
    /// 20  F0 00 00 00   end (unreachable)
    /// ```
    #[test]
    fn dumps_a_real_pattern_consistently() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        assert_eq!(pattern_length(&module, 0x08), Some(21));

        let mut runner = PatternRunner::new(&module, 0x08).expect("pattern in range");
        // Jiffy at which each entry ran, plus the entry itself.
        let mut log: Vec<(u32, u16, PatternEntry)> = Vec::new();
        for jiffy in 0..200 {
            let step = runner.step();
            runner
                .advance(|entry| log.push((jiffy, step, entry)))
                .expect("pattern stays in range");
        }

        // The first entry is the `$F3 01` wait, and it holds the runner for
        // two jiffies before the first note sounds.
        assert_eq!(
            log[0],
            (
                0,
                0,
                PatternEntry::Command(PatternCommand::Wait { jiffies: 1 })
            )
        );
        assert_eq!(
            log[1].2,
            PatternEntry::Note {
                note: 0x22,
                macro_number: 0x12,
                volume: 8,
                voice: 3,
                timing: NoteTiming::Detune(0),
            }
        );
        assert_eq!(log[1].0, 2);

        // Every note in this pattern triggers a macro on voice 3 -- one
        // pattern drives one voice, so a mis-split `cv` byte would show up
        // here immediately.
        let notes: Vec<_> = log
            .iter()
            .filter_map(|(jiffy, _, entry)| match entry {
                PatternEntry::Note { voice, timing, .. } => Some((*jiffy, *voice, *timing)),
                _ => None,
            })
            .collect();
        assert!(notes.iter().all(|(_, voice, _)| *voice == 3));
        // All of them are immediate-fetch notes: the timing comes from the
        // `$F3` commands between them.
        assert!(
            notes
                .iter()
                .all(|(_, _, timing)| matches!(timing, NoteTiming::Detune(0)))
        );

        // The `$F1` at step 19 loops back to step 1, so the pattern never
        // reaches its `$F0` and the runner never halts.
        assert_eq!(runner.halted(), None);
        assert!(
            log.iter()
                .all(|(_, _, entry)| !matches!(entry, PatternEntry::Command(PatternCommand::End)))
        );

        // The loop body is nine notes and exactly 32 jiffies long -- one bar
        // at four jiffies per sixteenth. Off-by-one wait accounting (`dd`
        // instead of `dd`+1, or a wait charged twice) would not land on 32.
        let loops: Vec<u32> = log
            .iter()
            .filter(|(_, _, entry)| {
                matches!(entry, PatternEntry::Command(PatternCommand::Loop { .. }))
            })
            .map(|(jiffy, _, _)| *jiffy)
            .collect();
        assert!(loops.len() >= 5, "expected several passes, got {loops:?}");
        assert!(loops.windows(2).all(|w| w[1] - w[0] == 32), "{loops:?}");
        assert_eq!(
            notes
                .iter()
                .filter(|(jiffy, _, _)| (loops[0]..loops[1]).contains(jiffy))
                .count(),
            9,
        );

        // The program counter never leaves the pattern's 21 longwords: the
        // `$F1` target is a step index into this pattern, so it stays in
        // bounds where a byte offset (76) or an absolute `mdat` offset would
        // not.
        assert!(log.iter().all(|(_, step, _)| *step < 21));
    }
}
