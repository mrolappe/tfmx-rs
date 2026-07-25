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
        number => TrackSlot::Pattern {
            number,
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
        let decoded = decode_line(self.module.trackstep_line(self.line)?);
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
}
