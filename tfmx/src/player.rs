//! `Player`: ties the trackstep runner, the eight pattern runners, the four
//! macro interpreters and `Paula` together behind one `render()` call.
//! `docs/architecture.md` §3.

use crate::macro_interp::{MacroEvent, MacroInterpreter, UnsupportedOps};
use crate::module::{AccessError, Module};
use crate::paula::Paula;
use crate::sequencer::{
    LineCommand, NoteTiming, PatternCommand, PatternEntry, PatternRunner, Sequencer, TickClock,
    TrackSlot, TrackstepLine,
};
use crate::trace::TraceEvent;

/// Paula has four hardware voices; a pattern note's `voice` nibble is
/// masked down to that range. **Uncertain**: [S1] never states that the
/// nibble is meant to stay within `0`-`3` -- `docs/format.md` §6 already
/// flags the field itself as unexplained beyond "the voice the macro runs
/// on". Masking rather than rejecting keeps a corpus file with a stray high
/// bit playable instead of erroring.
fn voice_of(nibble: u8) -> usize {
    (nibble & 0x03) as usize
}

/// How the eight tracks aggregate into the one shared trackstep line
/// pointer.
///
/// `docs/opcodes.md` §2 states the trigger at "documented" confidence --
/// `$F0 <End>`: *"Ends this pattern; trackstep advances."* -- so the line
/// pointer is driven by pattern completion, not by the tick clock. What no
/// source states is what happens when the eight tracks reach `$F0` at
/// different times, which `docs/playback-model.md` §7 recorded as open --
/// **now settled**: in the TFMX editor, authoring one track's pattern to a
/// fixed 2-jiffy length and another's to 200 jiffies on the same trackstep
/// line, the line advanced in both cases as soon as the *short* track hit
/// `$F0`, with zero effect from the long track's length. Only `AnyTrack`
/// predicts that. Both readings stay implemented for comparison, but
/// `AnyTrack` is the confirmed default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrackstepGate {
    /// Every track still running a pattern must have reached `$F0`. The
    /// line lasts as long as its longest track, which is what makes a
    /// pure-filler pattern (`$F3 <Wait>` then `$F0`, as `turrican intro`'s
    /// pattern 21 is in its entirety) a way to pad a short track out to the
    /// line's length. Ruled out by the editor test described above.
    AllTracks,
    /// Any one track reaching `$F0` moves the line, truncating the rest.
    /// The same filler pattern then reads as the line's *metronome*, fixing
    /// its length regardless of what the other tracks were still playing.
    /// Confirmed against the TFMX editor -- see above.
    #[default]
    AnyTrack,
}

/// Whether this jiffy may consume a trackstep line, per `gate`.
///
/// A track holds the line only while it is running a pattern that has not
/// yet reached `$F0 <End>`. `$F4 <STOP>` (and `$FE <StopCustom>`) take
/// their track out of the vote entirely: [S1] says a stopped track is
/// *"unrecoverable until a new pattern pointer is loaded; will not run any
/// upcoming `<End>`"* (`docs/opcodes.md` §2), so counting one as still
/// holding would stall the song forever. With nobody holding it the line
/// advances -- which is also what boots the very first line and what
/// carries `$EFFE` command lines, neither of which runs a pattern at all.
fn trackstep_line_due(patterns: &[Option<PatternRunner>; 8], gate: TrackstepGate) -> bool {
    let mut holding = 0usize;
    let mut ended = 0usize;
    for runner in patterns.iter().flatten() {
        match runner.halted() {
            Some(PatternCommand::End) => ended += 1,
            Some(_) => {}
            None => holding += 1,
        }
    }
    match gate {
        TrackstepGate::AllTracks => holding == 0,
        TrackstepGate::AnyTrack => holding == 0 || ended > 0,
    }
}

/// Owns the whole per-song playback state and exposes the single
/// `render()` entry point every caller (CLI, later a realtime backend)
/// drives. `docs/architecture.md` §3.
pub struct Player<'a> {
    module: &'a Module<'a>,
    smpl: &'a [i8],
    sequencer: Sequencer<'a>,
    patterns: [Option<PatternRunner<'a>>; 8],
    macros: [MacroInterpreter; 4],
    paula: Paula,
    unsupported: UnsupportedOps,
    clock: TickClock,
    /// Per-voice `$FD <Lock>` countdown, in jiffies remaining. `docs/
    /// opcodes.md` §2: while non-zero, `Note` entries targeting that voice
    /// are dropped rather than dispatched.
    lock: [u32; 4],
    /// Which tracks must reach `$F0 <End>` before the shared trackstep line
    /// pointer moves. See [`TrackstepGate`].
    trackstep_gate: TrackstepGate,
    sample_rate: u32,
    /// Total frames rendered across every `render`/`render_traced` call on
    /// this player -- gives [`TraceEvent::Jiffy`]'s `frame` a continuous,
    /// session-wide timeline even when a caller renders in chunks (as
    /// `tfmx-cli` does).
    frames_rendered: u64,
}

impl<'a> Player<'a> {
    /// A player for `song` (0-31), rendering at `sample_rate` Hz with
    /// `separation` (0-100, `docs/playback-model.md` §2.6) between Paula's
    /// hard-panned voices.
    pub fn new(
        module: &'a Module<'a>,
        song: u8,
        sample_rate: u32,
        separation: u8,
    ) -> Result<Self, AccessError> {
        let sequencer = Sequencer::new(module, song)?;
        let clock = TickClock::new(sequencer.tempo());
        // `Paula::new`'s own default (64, no attenuation) stands at song
        // start too: `docs/status.md`'s "Update (2026-07-26, later)" section
        // tried defaulting to 0 (a fade-in read of turrican intro's opening
        // slide) and found it falsified by the rest of the corpus --
        // `apidya (level 1)` (confirmed TFMX Pro, not the unrelated 7V
        // `apidya (title)`) never touches master volume and would render
        // permanently silent (`tfmx-cli lint` on it: peak amplitude 0)
        // despite ~280 real note-ons. A crate-wide default below 64 is
        // inconsistent with any module that doesn't manage it explicitly.
        Ok(Self {
            module,
            smpl: module.smpl(),
            sequencer,
            patterns: [None, None, None, None, None, None, None, None],
            macros: core::array::from_fn(|_| MacroInterpreter::new()),
            paula: Paula::new(separation),
            unsupported: UnsupportedOps::default(),
            clock,
            lock: [0; 4],
            trackstep_gate: TrackstepGate::default(),
            sample_rate,
            frames_rendered: 0,
        })
    }

    /// Opcodes this crate recognizes but does not implement, seen so far
    /// across every voice (`$1B`, `$22`-`$29`). `docs/opcodes.md` Unresolved.
    pub fn unsupported_ops(&self) -> &UnsupportedOps {
        &self.unsupported
    }

    /// Selects which tracks gate a trackstep line advance. See
    /// [`TrackstepGate`] -- the two readings are an open question this
    /// crate cannot settle from any published source, so the choice is the
    /// caller's to make and to listen to.
    pub fn set_trackstep_gate(&mut self, gate: TrackstepGate) {
        self.trackstep_gate = gate;
    }

    /// Mutes `voice` (0-3) at the mix; forwards to `Paula::set_voice_muted`.
    pub fn set_voice_muted(&mut self, voice: u8, muted: bool) {
        self.paula.set_voice_muted(voice, muted);
    }

    /// Fills `out` (interleaved stereo `i16`) with `out.len() / 2` frames,
    /// running the tick clock, the trackstep/pattern/macro state machines
    /// and the mixer together. `docs/architecture.md` §3.
    pub fn render(&mut self, out: &mut [i16]) -> Result<(), AccessError> {
        self.render_inner(out, |_| {})
    }

    /// As [`Player::render`], but also calls `trace` for every state-machine
    /// transition each jiffy produces -- the observation seam
    /// `docs/architecture.md` §2 documents alongside the register seam.
    /// `render()` is exactly this call with `|_| {}`, which the golden-hash
    /// regression tests prove monomorphizes away to identical output.
    pub fn render_traced(
        &mut self,
        out: &mut [i16],
        trace: impl FnMut(TraceEvent),
    ) -> Result<(), AccessError> {
        self.render_inner(out, trace)
    }

    fn render_inner(
        &mut self,
        out: &mut [i16],
        mut trace: impl FnMut(TraceEvent),
    ) -> Result<(), AccessError> {
        let frames = (out.len() / 2) as u32;
        let Player {
            module,
            smpl,
            sequencer,
            patterns,
            macros,
            paula,
            unsupported,
            clock,
            lock,
            trackstep_gate,
            sample_rate,
            frames_rendered,
        } = self;
        let mut pos = 0usize;
        let mut error = None;
        clock.advance(*sample_rate, frames, |tick_due, chunk_frames| {
            if tick_due
                && error.is_none()
                && let Err(e) = run_jiffy(
                    module,
                    sequencer,
                    patterns,
                    macros,
                    paula,
                    unsupported,
                    lock,
                    *trackstep_gate,
                    *frames_rendered + pos as u64,
                    &mut trace,
                )
            {
                error = Some(e);
            }
            let start = pos * 2;
            let end = start + chunk_frames as usize * 2;
            paula.render(smpl, *sample_rate, &mut out[start..end]);
            pos += chunk_frames as usize;
        });
        *frames_rendered += frames as u64;
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Advances every state machine by exactly one jiffy: the trackstep line
/// (see the module-level note on why this runs unconditionally, not gated
/// on any pattern reaching `$F0 <End>`), then each track's pattern one
/// step, then each voice's macro program, in the signal-chain order
/// `docs/playback-model.md` §1 lists.
// Each parameter is one of `Player`'s own fields, already destructured by
// `render_inner`'s only caller -- bundling them into a struct here would
// just re-wrap what the caller already unwrapped.
#[allow(clippy::too_many_arguments)]
fn run_jiffy<'a>(
    module: &'a Module<'a>,
    sequencer: &mut Sequencer<'a>,
    patterns: &mut [Option<PatternRunner<'a>>; 8],
    macros: &mut [MacroInterpreter; 4],
    paula: &mut Paula,
    unsupported: &mut UnsupportedOps,
    lock: &mut [u32; 4],
    gate: TrackstepGate,
    frame: u64,
    trace: &mut impl FnMut(TraceEvent),
) -> Result<(), AccessError> {
    trace(TraceEvent::Jiffy {
        frame,
        line: sequencer.current_line(),
        tempo: sequencer.tempo(),
        stopped: sequencer.is_stopped(),
    });

    // `docs/opcodes.md` §2, at "documented" confidence: `$F0 <End>` --
    // "Ends this pattern; trackstep advances." The line pointer is driven
    // by pattern completion, not by the tick clock; `trackstep_line_due`
    // holds it while any track is still playing. Only the aggregation
    // across the eight tracks is unstated (`docs/playback-model.md` §7),
    // which is what `gate` selects between.
    let mut assigned = None;
    if !sequencer.is_stopped() && trackstep_line_due(patterns, gate) {
        let line = sequencer.advance()?;
        match &line {
            TrackstepLine::Command(
                LineCommand::MasterVolSlideA { divisor, target }
                | LineCommand::MasterVolSlideB { divisor, target },
            ) => paula.start_master_volume_slide(*divisor as u8, *target as u8),
            TrackstepLine::Tracks(slots) => assigned = Some(*slots),
            TrackstepLine::Command(_) => {}
        }
        trace(TraceEvent::Trackstep(line));
    }

    // Track words are per-line data, so they are applied on the jiffy their
    // line is consumed and not again until the next one. An explicit
    // pattern word (re)starts that track's pattern -- restarting is the
    // point, since the line only came up because the old one ended. `$80
    // <Hold>` deliberately does not touch the runner: "keep the currently
    // running pattern; only the transpose changes" (`docs/format.md` §5),
    // which is also what keeps a `$FB <PPat>` jump this track has taken
    // since from being silently undone.
    if let Some(slots) = assigned {
        for (i, slot) in slots.into_iter().enumerate() {
            match slot {
                TrackSlot::Pattern { number, .. } => {
                    patterns[i] = Some(PatternRunner::new(module, number)?);
                }
                TrackSlot::Hold { .. } => {}
                TrackSlot::StopChannel => patterns[i] = None,
                TrackSlot::StopVoice { voice } => macros[voice_of(voice)].stop_voice(),
            }
        }
    }

    // `$FB <PPat>` jumps to a track that may be earlier or later than the
    // one issuing it in this same 0..7 pass -- collected here and applied
    // only after every track has run this jiffy (see `dispatch_pattern_
    // entry`'s doc comment for why that single ordering covers both of
    // `docs/opcodes.md` §2's "immediate"/"next entry" cases for free).
    let mut pattern_jumps: [Option<u8>; 8] = [None; 8];
    for i in 0..8u8 {
        let transpose = match sequencer.track(i) {
            TrackSlot::Pattern { transpose, .. } | TrackSlot::Hold { transpose } => transpose,
            _ => 0,
        };
        if let Some(runner) = &mut patterns[i as usize] {
            runner.advance(|pattern, step, entry| {
                trace(TraceEvent::Pattern {
                    track: i,
                    pattern,
                    step,
                    entry,
                });
                if let Some((target_track, target_pattern)) =
                    dispatch_pattern_entry(entry, transpose, macros, paula, lock, trace)
                {
                    pattern_jumps[target_track as usize] = Some(target_pattern);
                }
            })?;
        }
    }
    for (track, pattern) in pattern_jumps.into_iter().enumerate() {
        if let Some(pattern) = pattern {
            patterns[track] = Some(PatternRunner::new(module, pattern)?);
        }
    }
    tick_locks(lock);

    let mut play_macro_events = [None; 4];
    for (voice, mac) in macros.iter_mut().enumerate() {
        let mut event = None;
        mac.tick(module, paula, voice as u8, unsupported, |e| event = Some(e))?;
        play_macro_events[voice] = event;
    }
    for event in play_macro_events.into_iter().flatten() {
        let MacroEvent::PlayMacro {
            channel,
            macro_number,
            detune,
        } = event;
        macros[voice_of(channel)].play_macro(macro_number, detune);
    }

    paula.tick_master_volume();

    for voice in 0..4u8 {
        trace(TraceEvent::Voice {
            voice,
            state: paula.voice(voice),
        });
    }

    Ok(())
}

/// Decrements every voice's `$FD <Lock>` countdown by one jiffy, floored at
/// 0 -- called once per jiffy, after that jiffy's own pattern dispatch (so
/// the jiffy a `Lock` command runs on still fully blocks other notes on
/// that voice).
fn tick_locks(lock: &mut [u32; 4]) {
    for remaining in lock {
        *remaining = remaining.saturating_sub(1);
    }
}

/// Routes one decoded pattern longword to the voice it names. Returns the
/// `(track, pattern)` of a `$FB <PPat>` cross-track jump, if this entry was
/// one -- the caller applies it after every track has had its turn this
/// jiffy (see `run_jiffy`'s own comment on why that single-pass order is
/// exactly what `docs/opcodes.md` §2's "own track lower than target track"
/// timing rule needs, with no extra bookkeeping). `docs/opcodes.md` §2-§3.
fn dispatch_pattern_entry(
    entry: PatternEntry,
    transpose: i8,
    macros: &mut [MacroInterpreter; 4],
    paula: &mut Paula,
    lock: &mut [u32; 4],
    trace: &mut impl FnMut(TraceEvent),
) -> Option<(u8, u8)> {
    match entry {
        PatternEntry::Note {
            note,
            macro_number,
            volume,
            voice,
            timing,
        } => {
            let voice = voice_of(voice) as u8;
            // `$FD <Lock>`: "locks channel against other notes" -- a note
            // for a still-locked voice is dropped, not deferred or queued.
            if lock[voice as usize] > 0 {
                return None;
            }
            // Only a note byte below `$80` carries a finetune in `dd`; the
            // `Wait`/`Portamento` forms spend that byte on timing instead.
            // `docs/playback-model.md` §4.
            let detune = match timing {
                NoteTiming::Detune(detune) => detune,
                NoteTiming::Wait(_) | NoteTiming::Portamento(_) => 0,
            };
            macros[voice as usize].note_on(macro_number, note, volume, transpose, detune);
            trace(TraceEvent::Trigger {
                voice,
                macro_number,
                note,
                volume,
                transpose,
            });
            None
        }
        PatternEntry::Command(command) => match command {
            PatternCommand::KeyUp { voice } => {
                macros[voice_of(voice)].signal_key_up();
                None
            }
            PatternCommand::Vibrato {
                speed,
                voice,
                depth,
            } => {
                macros[voice_of(voice)].start_vibrato(speed, depth as i8);
                None
            }
            PatternCommand::Envelope {
                amount,
                speed,
                voice,
                target,
            } => {
                // "$F7 <Enve>: every b+1 jiffies" -- `docs/opcodes.md` §2,
                // unlike macro $0F's own "every bb jiffies" with no +1.
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
            // `transpose` is decoded (`sequencer.rs`) but not applied here:
            // [S1] gives PPat's jumped-to track the same (pattern, transpose)
            // shape as a trackstep `Pattern` slot, but that slot's transpose
            // is re-supplied fresh from the trackstep table every jiffy
            // (`run_jiffy`'s `transpose` local) independent of any pattern-
            // level command, and [S1] never states which one wins on a live
            // track. Recorded as a known partial gap rather than guessed.
            PatternCommand::PlayPattern {
                pattern, track, ..
            } => Some((track & 0x07, pattern)),
            PatternCommand::End
            | PatternCommand::Loop { .. }
            | PatternCommand::Jump { .. }
            | PatternCommand::Wait { .. }
            | PatternCommand::Stop
            | PatternCommand::GoSub { .. }
            | PatternCommand::Return
            | PatternCommand::StopCustom
            | PatternCommand::Nop => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencer::NoteTiming;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(path).ok()
    }

    /// The step's own check: a 30-second render of a real corpus song is
    /// not silent, produces no `NaN`/`inf`-tainted output, isn't wall-to-
    /// wall full-scale clipping, and is bit-identical across two
    /// independent runs -- rendered in different block sizes, so this also
    /// exercises the block-size independence step 4.1 established.
    #[test]
    fn thirty_second_render_is_sane_and_reproducible() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        const SAMPLE_RATE: u32 = 44100;
        const SECONDS: usize = 30;
        let total_frames = SAMPLE_RATE as usize * SECONDS;

        let mut one_call = vec![0i16; total_frames * 2];
        let mut player = Player::new(&module, 0, SAMPLE_RATE, 100).expect("song 0 in range");
        player
            .render(&mut one_call)
            .expect("every trackstep/pattern/macro access stays in range");

        let mut chunked = vec![0i16; total_frames * 2];
        let mut player = Player::new(&module, 0, SAMPLE_RATE, 100).expect("song 0 in range");
        for block in chunked.chunks_mut(997 * 2) {
            player.render(block).expect("stays in range, chunked too");
        }

        // `i16` output can't carry a `NaN`/`inf` -- `Paula::render` clamps
        // through `f64::clamp` before casting -- so there is no separate
        // "not NaN/inf" assertion to write at this type; a rogue division
        // upstream would show up as the silence or clipping checks below.
        assert!(
            one_call.iter().any(|&s| s != 0),
            "render must not be silent"
        );
        let clipped = one_call
            .iter()
            .filter(|&&s| s == i16::MIN || s == i16::MAX)
            .count();
        assert!(
            clipped < one_call.len() / 2,
            "output looks like wall-to-wall clipping ({clipped}/{} samples at full scale)",
            one_call.len()
        );

        assert_eq!(
            one_call, chunked,
            "one 30s render call must be bit-identical to many small chunks"
        );
    }

    /// Step 11.3's load-bearing proof that the trace seam is inert:
    /// `render()` is literally `render_traced(.., |_| {})`, so a no-op trace
    /// closure must produce byte-identical output to `render()`.
    #[test]
    fn render_traced_with_a_no_op_trace_matches_render() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        const SAMPLE_RATE: u32 = 44100;
        const SECONDS: usize = 5;
        let total_frames = SAMPLE_RATE as usize * SECONDS;

        let mut plain = vec![0i16; total_frames * 2];
        let mut player = Player::new(&module, 0, SAMPLE_RATE, 100).expect("song 0 in range");
        player.render(&mut plain).expect("stays in range");

        let mut traced = vec![0i16; total_frames * 2];
        let mut player = Player::new(&module, 0, SAMPLE_RATE, 100).expect("song 0 in range");
        player
            .render_traced(&mut traced, |_| {})
            .expect("stays in range");

        assert_eq!(
            plain, traced,
            "render_traced with a no-op trace must be bit-identical to render()"
        );
    }

    /// Step 11.3's own check: a 1s traced render emits roughly one `Jiffy`
    /// event per 50 Hz tick, with `frame` never going backwards. Song 1 of
    /// `turrican intro` runs at tempo 120 (the CIA/BPM path, `docs/
    /// playback-model.md` §3.2: `tick_rate_hz = 120 * 24 / 60` = 48 Hz) --
    /// song 0's tempo 3 is the slow 50 Hz-divider path (12.5 Hz) and would
    /// not land in the "~50" range this check names.
    #[test]
    fn traced_render_emits_jiffy_events_with_monotonic_frame() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        const SAMPLE_RATE: u32 = 44100;
        let mut out = vec![0i16; SAMPLE_RATE as usize * 2];
        let mut player = Player::new(&module, 1, SAMPLE_RATE, 100).expect("song 1 in range");

        let mut jiffy_frames = Vec::new();
        player
            .render_traced(&mut out, |event| {
                if let TraceEvent::Jiffy { frame, .. } = event {
                    jiffy_frames.push(frame);
                }
            })
            .expect("stays in range");

        assert!(
            (40..=60).contains(&jiffy_frames.len()),
            "expected ~50 Jiffy events in 1s, got {}",
            jiffy_frames.len()
        );
        assert!(
            jiffy_frames.windows(2).all(|w| w[0] <= w[1]),
            "Jiffy frame must never go backwards: {jiffy_frames:?}"
        );
    }

    /// `frames_rendered` must keep advancing across separate `render_traced`
    /// calls, not reset to 0 -- otherwise a chunked trace (as `tfmx-cli`
    /// renders) would report the same frame range over and over.
    #[test]
    fn jiffy_frame_keeps_advancing_across_separate_render_calls() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        const SAMPLE_RATE: u32 = 44100;
        let mut player = Player::new(&module, 0, SAMPLE_RATE, 100).expect("song 0 in range");

        let mut last_frame = None;
        for _ in 0..10 {
            let mut out = vec![0i16; 4096 * 2];
            player
                .render_traced(&mut out, |event| {
                    if let TraceEvent::Jiffy { frame, .. } = event {
                        last_frame = Some(frame);
                    }
                })
                .expect("stays in range");
        }

        // 10 chunks of 4096 frames = 40960 frames total, so a correctly
        // accumulating counter must land well past a single chunk's worth
        // (4096) -- if `frames_rendered` reset every call instead, the last
        // observed frame could never exceed one chunk's size.
        assert!(
            last_frame.unwrap() > 4096 * 9,
            "frame should keep advancing across chunked calls, not reset each call, got {last_frame:?}"
        );
    }

    /// `docs/status.md`'s "Update (2026-07-26, later)" section tried
    /// defaulting song start to 0 and found it falsified: `apidya (level 1)`
    /// (confirmed TFMX Pro) never touches master volume and would render
    /// permanently silent under that policy despite real note-ons. Song
    /// start now stands on `Paula::new`'s own neutral default.
    #[test]
    fn player_new_defaults_master_volume_to_full_scale() {
        let mut mdat = vec![0u8; 0x800];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");
        assert_eq!(player.paula.master_volume(), 64);
    }

    /// End-to-end proof that a trackstep `$EFFE 0003`/`0004` line is applied
    /// to `Paula`: a synthetic one-line module sliding down to 0 (divisor 0,
    /// so `docs/playback-model.md` §5.1's shared envelope mechanic moves by
    /// 1 every jiffy with no waiting). Deliberately not `turrican intro`'s
    /// own real slide, which targets 64 from a default of 64 and is
    /// therefore a no-op -- this proves the wiring independent of that now-
    /// understood-inert real-world data point.
    #[test]
    fn trackstep_master_vol_slide_moves_on_the_first_jiffy() {
        const STOP_LINE: [u8; 16] = [
            0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
            0xFF, 0x00,
        ];
        let mut effe_line = [0u8; 16];
        effe_line[0..2].copy_from_slice(&0xEFFEu16.to_be_bytes()); // $EFFE
        effe_line[2..4].copy_from_slice(&0x0003u16.to_be_bytes()); // MasterVolSlideA
        effe_line[4..6].copy_from_slice(&0u16.to_be_bytes()); // divisor 0
        effe_line[6..8].copy_from_slice(&0u16.to_be_bytes()); // target 0

        let mut mdat = vec![0u8; 0x800 + 2 * 16];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x140..0x142].copy_from_slice(&1u16.to_be_bytes()); // song_end
        mdat[0x180..0x182].copy_from_slice(&1u16.to_be_bytes()); // tempo
        mdat[0x800..0x810].copy_from_slice(&effe_line);
        mdat[0x810..0x820].copy_from_slice(&STOP_LINE);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");

        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");
        assert_eq!(player.paula.master_volume(), 64);

        let mut out = vec![0i16; 2]; // one frame is enough: the first tick is due immediately
        player.render(&mut out).expect("stays in range");

        assert_eq!(
            player.paula.master_volume(),
            63,
            "divisor 0 moves master volume by 1 on the very first jiffy"
        );
    }

    /// Pattern `$FA <Fade>` was recognized and timed but never consumed --
    /// same bucket as `PlayPattern`/`Lock` until now. Unlike those, this one
    /// is fixed: it must start the shared master-volume slide on `Paula`.
    #[test]
    fn fade_pattern_command_starts_a_master_volume_slide() {
        let mut macros = core::array::from_fn(|_| MacroInterpreter::new());
        let mut paula = Paula::new(100);
        paula.set_master_volume(0);

        dispatch_pattern_entry(
            PatternEntry::Command(PatternCommand::Fade {
                speed: 1,
                target: 40,
            }),
            0,
            &mut macros,
            &mut paula,
            &mut [0; 4],
            &mut |_| {},
        );
        paula.tick_master_volume();
        paula.tick_master_volume();

        assert_eq!(paula.master_volume(), 2);
    }

    /// `$FD <Lock>`: "locks channel `aa`&3 against other notes for `bbbb`
    /// ticks" (`docs/opcodes.md` §2). A `Note` entry for the locked voice
    /// dispatched while the lock is in effect must not reach the macro
    /// interpreter at all.
    #[test]
    fn lock_pattern_command_blocks_a_same_voice_note_dispatched_after_it() {
        let mut macros = core::array::from_fn(|_| MacroInterpreter::new());
        let mut paula = Paula::new(100);
        let mut lock = [0u32; 4];

        dispatch_pattern_entry(
            PatternEntry::Command(PatternCommand::Lock {
                channel: 2,
                ticks: 3,
            }),
            0,
            &mut macros,
            &mut paula,
            &mut lock,
            &mut |_| {},
        );
        assert_eq!(lock[2], 3, "Lock must arm the counter for its own channel");

        dispatch_pattern_entry(
            PatternEntry::Note {
                note: 30,
                macro_number: 7,
                volume: 15,
                voice: 2,
                timing: NoteTiming::Detune(0),
            },
            0,
            &mut macros,
            &mut paula,
            &mut lock,
            &mut |_| {},
        );

        assert_eq!(
            macros[2].macro_number(),
            0,
            "a Note for a locked voice must never reach note_on"
        );
    }

    /// The pattern note record's `dd` byte is a finetune when the note byte
    /// is below `$80` (`docs/playback-model.md` §4) -- `dispatch_pattern_
    /// entry` must hand it to the macro interpreter, not drop it.
    #[test]
    fn pattern_record_detune_reaches_the_voices_period() {
        // Macro 0: `$09 <SetNote> $1E` with its own finetune 0, then `$07`.
        let mut mdat = vec![0u8; 0x900];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x600..0x604].copy_from_slice(&0x900u32.to_be_bytes());
        mdat.extend_from_slice(&[0x09, 0x1E, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");

        let mut macros = core::array::from_fn(|_| MacroInterpreter::new());
        let mut paula = Paula::new(100);
        dispatch_pattern_entry(
            PatternEntry::Note {
                note: 0x1E,
                macro_number: 0,
                volume: 15,
                voice: 0,
                timing: NoteTiming::Detune(0x40),
            },
            0,
            &mut macros,
            &mut paula,
            &mut [0; 4],
            &mut |_| {},
        );
        let mut unsupported = UnsupportedOps::default();
        macros[0]
            .tick(&module, &mut paula, 0, &mut unsupported, |_| {})
            .expect("stays in range");

        assert_eq!(
            paula.voice(0).period,
            crate::macro_interp::note_period(0x1E, 0x40),
            "the pattern record's detune must reach the period"
        );
    }

    /// A locked voice must accept notes again once the counter reaches 0,
    /// and a lock on one voice must never block a different voice.
    #[test]
    fn tick_locks_counts_down_and_stops_blocking_at_zero() {
        let mut lock = [0u32, 2, 0, 0];
        tick_locks(&mut lock);
        assert_eq!(lock, [0, 1, 0, 0]);
        tick_locks(&mut lock);
        assert_eq!(lock, [0, 0, 0, 0], "must not underflow past 0");
        tick_locks(&mut lock);
        assert_eq!(lock, [0, 0, 0, 0]);
    }

    /// End-to-end proof of `docs/opcodes.md` §2's `$FB <PPat>` timing rule
    /// via a synthetic two-track module: track 0's pattern issues
    /// `PlayPattern(pattern: 2, track: 1)` then stops for the rest of the
    /// jiffy; track 1 (index 1, greater than track 0's own index 0) starts
    /// on pattern 1. "own track number lower than target track: takes
    /// effect on the next entry into the play routine" means track 1's
    /// dispatch *this same jiffy* must still run its old pattern 1 (macro
    /// 5); only the *following* jiffy must show pattern 2 (macro 9).
    #[test]
    fn play_pattern_command_redirects_the_named_track_on_the_next_jiffy() {
        let pattern0: [u8; 8] = [
            0xFB, 0x02, 0x01, 0x00, // PlayPattern(pattern=2, track=1, transpose=0)
            0xF3, 0x00, 0x00, 0x00, // Wait(0): stop for the rest of this jiffy
        ];
        let pattern1: [u8; 4] = [0x80, 0x05, 0xF1, 0x00]; // Note(macro=5, voice=1, Wait(0))
        let pattern2: [u8; 4] = [0x80, 0x09, 0xF1, 0x00]; // Note(macro=9, voice=1, Wait(0))
        let macro_program: [u8; 4] = [0x00, 0x00, 0x00, 0x00]; // $00 aa=0: pause, suspend

        let mut mdat = vec![0u8; 0x900];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x140..0x142].copy_from_slice(&1u16.to_be_bytes()); // song_end = line 1
        mdat[0x180..0x182].copy_from_slice(&1u16.to_be_bytes()); // tempo

        const PATTERN_TABLE: usize = 0x400;
        const MACRO_TABLE: usize = 0x600;
        let mut offset = 0x900u32;
        for (slot, len) in [(0usize, 8u32), (1, 4), (2, 4)] {
            mdat[PATTERN_TABLE + slot * 4..PATTERN_TABLE + slot * 4 + 4]
                .copy_from_slice(&offset.to_be_bytes());
            offset += len;
        }
        mdat.extend_from_slice(&pattern0);
        mdat.extend_from_slice(&pattern1);
        mdat.extend_from_slice(&pattern2);

        let macro_offset = offset;
        for slot in [5usize, 9] {
            mdat[MACRO_TABLE + slot * 4..MACRO_TABLE + slot * 4 + 4]
                .copy_from_slice(&macro_offset.to_be_bytes());
        }
        mdat.extend_from_slice(&macro_program);

        // Line 0: track 0 -> pattern 0, track 1 -> pattern 1, rest stopped.
        let mut line0 = [0u8; 16];
        line0[0..2].copy_from_slice(&0x0000u16.to_be_bytes());
        line0[2..4].copy_from_slice(&0x0100u16.to_be_bytes());
        for w in 2..8 {
            line0[w * 2..w * 2 + 2].copy_from_slice(&0xFF00u16.to_be_bytes());
        }
        // Line 1: hold both tracks -- a fresh `Pattern{number: 1, ..}` here
        // would make `run_jiffy`'s reload check stomp the jump's redirect
        // right back to pattern 1 before track 1 ever got to use it.
        let mut line1 = [0u8; 16];
        line1[0..2].copy_from_slice(&0x8000u16.to_be_bytes());
        line1[2..4].copy_from_slice(&0x8000u16.to_be_bytes());
        for w in 2..8 {
            line1[w * 2..w * 2 + 2].copy_from_slice(&0xFF00u16.to_be_bytes());
        }
        mdat[0x800..0x810].copy_from_slice(&line0);
        mdat[0x810..0x820].copy_from_slice(&line1);

        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");

        let mut out = vec![0i16; 2]; // one frame is enough: the first tick is due immediately
        player.render(&mut out).expect("jiffy 0 stays in range");
        assert_eq!(
            player.macros[1].macro_number(),
            5,
            "track 1 must still run its old pattern's note the same jiffy the jump was issued"
        );

        // Tempo 1 -> 25 Hz -> 1764 samples/jiffy (`tick_fraction`); one
        // frame already consumed the first tick, so 1764 more crosses
        // exactly into the second without reaching a third.
        let mut out2 = vec![0i16; 1764 * 2];
        player.render(&mut out2).expect("jiffy 1 stays in range");
        assert_eq!(
            player.macros[1].macro_number(),
            9,
            "track 1 must run the jumped-to pattern's note starting the next jiffy"
        );
    }

    // -- `$F0 <End>` trackstep gating --------------------------------------
    //
    // `docs/opcodes.md` §2 states the trigger outright, at "documented"
    // confidence: `$F0 <End>` -- "Ends this pattern; trackstep advances."
    // What it does not state, and what `docs/playback-model.md` §7 records
    // as open, is how the eight tracks aggregate. Both readings are built
    // and tested here so they can be A/B'd against the TFMX editor.

    /// One trackstep line from eight raw track words: `$00nn`-`$7Fnn` =
    /// pattern `nn` (low byte transpose), `$80nn` = Hold, `$FFxx` =
    /// StopChannel (`sequencer::decode_track_word`).
    fn trackstep_line(words: [u16; 8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        for (i, word) in words.iter().enumerate() {
            out[i * 2..i * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// A synthetic module: `patterns[i]` becomes pattern `i`, `lines`
    /// becomes trackstep lines `0..lines.len()` (song 0 spans all of them),
    /// and every macro slot `0..16` points at one `$00` pause program --
    /// enough to observe *which* macro number a note dispatched without any
    /// of them producing sound.
    fn synth_module(patterns: &[&[u8]], lines: &[[u8; 16]]) -> Vec<u8> {
        const PATTERN_TABLE: usize = 0x400;
        const MACRO_TABLE: usize = 0x600;

        let mut mdat = vec![0u8; 0x800 + lines.len() * 16];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x140..0x142].copy_from_slice(&(lines.len() as u16 - 1).to_be_bytes()); // song_end
        mdat[0x180..0x182].copy_from_slice(&1u16.to_be_bytes()); // tempo 1 -> 25 Hz
        for (i, line) in lines.iter().enumerate() {
            mdat[0x800 + i * 16..0x800 + (i + 1) * 16].copy_from_slice(line);
        }

        let mut offset = mdat.len() as u32;
        for (slot, data) in patterns.iter().enumerate() {
            mdat[PATTERN_TABLE + slot * 4..PATTERN_TABLE + slot * 4 + 4]
                .copy_from_slice(&offset.to_be_bytes());
            offset += data.len() as u32;
        }
        for data in patterns {
            mdat.extend_from_slice(data);
        }

        let macro_offset = mdat.len() as u32;
        for slot in 0..16usize {
            mdat[MACRO_TABLE + slot * 4..MACRO_TABLE + slot * 4 + 4]
                .copy_from_slice(&macro_offset.to_be_bytes());
        }
        // `$00 aa=0` (pause, suspends for one jiffy) then `$07 <STOP>`, so
        // the interpreter halts inside its own program instead of reading
        // off the end of `mdat` on the jiffy after it starts.
        mdat.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00]);
        mdat
    }

    /// A note longword with `Wait` timing: `$80|note`, macro, `cv`, wait.
    const fn note_word(macro_number: u8, voice: u8, wait: u8) -> [u8; 4] {
        [0x80, macro_number, voice, wait]
    }

    const END: [u8; 4] = [0xF0, 0x00, 0x00, 0x00];
    const STOP: [u8; 4] = [0xF4, 0x00, 0x00, 0x00];

    /// Renders exactly one jiffy of a [`synth_module`] (tempo 1 -> 25 Hz ->
    /// 1764 frames per tick, and the clock leaves the boundary exactly at
    /// the end of the block, so one call is one jiffy from the first on).
    fn jiffy(player: &mut Player) {
        let mut out = vec![0i16; 1764 * 2];
        player.render(&mut out).expect("stays in range");
    }

    /// The core of the rule, independent of how tracks aggregate: a line
    /// holds while its pattern is still running. Track 0's pattern waits 3
    /// jiffies after its note before reaching `$F0`, so the next line's
    /// note cannot be dispatched until jiffy 5.
    #[test]
    fn trackstep_line_holds_until_its_pattern_reaches_end() {
        let mut pattern0 = note_word(5, 0, 3).to_vec();
        pattern0.extend_from_slice(&END);
        let mut pattern1 = note_word(9, 0, 0).to_vec();
        pattern1.extend_from_slice(&END);

        let mdat = synth_module(
            &[&pattern0, &pattern1],
            &[
                trackstep_line([0x0000, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
                trackstep_line([0x0100, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
            ],
        );
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");

        // jiffy 0 loads line 0; jiffies 1-3 are the note's own wait; jiffy 4
        // fetches `$F0`. Only jiffy 5 may consume line 1.
        for expected_jiffy in 0..5 {
            jiffy(&mut player);
            assert_eq!(
                player.macros[0].macro_number(),
                5,
                "line 0's pattern is still running at jiffy {expected_jiffy}; \
                 the trackstep line must not have advanced yet"
            );
        }
        jiffy(&mut player);
        assert_eq!(
            player.macros[0].macro_number(),
            9,
            "the jiffy after `$F0 <End>`, the line advances and line 1's note runs"
        );
    }

    /// Two tracks whose patterns end at different jiffies. Under
    /// [`TrackstepGate::AllTracks`] the later one governs.
    #[test]
    fn all_tracks_gate_waits_for_the_last_track_to_end() {
        let module_bytes = two_track_module();
        let module = Module::parse(&module_bytes, &[]).expect("valid header parses");
        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");
        player.set_trackstep_gate(TrackstepGate::AllTracks);

        for expected_jiffy in 0..5 {
            jiffy(&mut player);
            assert_eq!(
                player.macros[0].macro_number(),
                5,
                "track 1's pattern runs until jiffy 4, so at jiffy {expected_jiffy} \
                 line 1 cannot have started yet"
            );
        }
        jiffy(&mut player);
        assert_eq!(
            player.macros[0].macro_number(),
            9,
            "once every track has reached `$F0`, the line advances"
        );
    }

    /// The same module under [`TrackstepGate::AnyTrack`]: track 0 reaches
    /// `$F0` at jiffy 1, so the line moves at jiffy 2 and truncates track 1.
    #[test]
    fn any_track_gate_advances_on_the_first_track_to_end() {
        let module_bytes = two_track_module();
        let module = Module::parse(&module_bytes, &[]).expect("valid header parses");
        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");
        player.set_trackstep_gate(TrackstepGate::AnyTrack);

        jiffy(&mut player);
        jiffy(&mut player);
        assert_eq!(
            player.macros[0].macro_number(),
            5,
            "track 0 only reaches `$F0` on jiffy 1; the line cannot have moved before jiffy 2"
        );
        jiffy(&mut player);
        assert_eq!(
            player.macros[0].macro_number(),
            9,
            "one track's `$F0` is enough under this reading, even with track 1 still running"
        );
    }

    /// Track 0 ends at jiffy 1 (`Wait(0)` then `$F0`), track 1 at jiffy 4
    /// (`Wait(3)` then `$F0`); line 1 puts a distinguishable macro on
    /// voice 0.
    fn two_track_module() -> Vec<u8> {
        let mut pattern0 = note_word(5, 0, 0).to_vec();
        pattern0.extend_from_slice(&END);
        let mut pattern1 = note_word(6, 1, 3).to_vec();
        pattern1.extend_from_slice(&END);
        let mut pattern2 = note_word(9, 0, 0).to_vec();
        pattern2.extend_from_slice(&END);

        synth_module(
            &[&pattern0, &pattern1, &pattern2],
            &[
                trackstep_line([0x0000, 0x0100, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
                trackstep_line([0x0200, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
            ],
        )
    }

    /// `$F4 <STOP>` is documented as "unrecoverable until a new pattern
    /// pointer is loaded; **will not run any upcoming `<End>`**"
    /// (`docs/opcodes.md` §2) -- so a track sitting on one can never cast a
    /// vote, and must drop out of the gate rather than hold the line for
    /// the rest of the song.
    #[test]
    fn a_stopped_track_does_not_hold_the_trackstep_line_forever() {
        let pattern0 = STOP.to_vec();
        let mut pattern1 = note_word(6, 1, 1).to_vec();
        pattern1.extend_from_slice(&END);
        let mut pattern2 = note_word(9, 0, 0).to_vec();
        pattern2.extend_from_slice(&END);

        let mdat = synth_module(
            &[&pattern0, &pattern1, &pattern2],
            &[
                trackstep_line([0x0000, 0x0100, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
                trackstep_line([0x0200, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
            ],
        );
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");
        player.set_trackstep_gate(TrackstepGate::AllTracks);

        for _ in 0..3 {
            jiffy(&mut player);
        }
        assert_eq!(
            player.macros[0].macro_number(),
            0,
            "track 1 still holds the line through jiffy 2"
        );
        jiffy(&mut player);
        assert_eq!(
            player.macros[0].macro_number(),
            9,
            "track 0's `$F4 <STOP>` must not veto the advance track 1 has earned"
        );
    }

    /// A line that stops every track leaves nobody to reach `$F0`. It must
    /// fall through to the next line rather than deadlock -- the same
    /// no-one-is-holding-it path that boots the very first line and that
    /// carries `$EFFE` command lines.
    #[test]
    fn a_line_with_no_running_pattern_advances_immediately() {
        let mut pattern0 = note_word(9, 0, 0).to_vec();
        pattern0.extend_from_slice(&END);

        let mdat = synth_module(
            &[&pattern0],
            &[
                trackstep_line([0xFF00; 8]),
                trackstep_line([0x0000, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00, 0xFF00]),
            ],
        );
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut player = Player::new(&module, 0, 44100, 100).expect("song 0 in range");

        jiffy(&mut player);
        jiffy(&mut player);
        assert_eq!(
            player.macros[0].macro_number(),
            9,
            "an all-StopChannel line must not stall the song"
        );
    }
}
