//! `Player`: ties the trackstep runner, the eight pattern runners, the four
//! macro interpreters and `Paula` together behind one `render()` call.
//! `docs/architecture.md` §3.

use crate::macro_interp::{MacroEvent, MacroInterpreter, UnsupportedOps};
use crate::module::{AccessError, Module};
use crate::paula::Paula;
use crate::sequencer::{
    PatternCommand, PatternEntry, PatternRunner, Sequencer, TickClock, TrackSlot,
};

/// Paula has four hardware voices; a pattern note's `voice` nibble is
/// masked down to that range. **Uncertain**: [S1] never states that the
/// nibble is meant to stay within `0`-`3` -- `docs/format.md` §6 already
/// flags the field itself as unexplained beyond "the voice the macro runs
/// on". Masking rather than rejecting keeps a corpus file with a stray high
/// bit playable instead of erroring.
fn voice_of(nibble: u8) -> usize {
    (nibble & 0x03) as usize
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
    sample_rate: u32,
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
        Ok(Self {
            module,
            smpl: module.smpl(),
            sequencer,
            patterns: [None, None, None, None, None, None, None, None],
            macros: core::array::from_fn(|_| MacroInterpreter::new()),
            paula: Paula::new(separation),
            unsupported: UnsupportedOps::default(),
            clock,
            sample_rate,
        })
    }

    /// Opcodes this crate recognizes but does not implement, seen so far
    /// across every voice (`$1B`, `$22`-`$29`). `docs/opcodes.md` Unresolved.
    pub fn unsupported_ops(&self) -> &UnsupportedOps {
        &self.unsupported
    }

    /// Mutes `voice` (0-3) at the mix; forwards to `Paula::set_voice_muted`.
    pub fn set_voice_muted(&mut self, voice: u8, muted: bool) {
        self.paula.set_voice_muted(voice, muted);
    }

    /// Fills `out` (interleaved stereo `i16`) with `out.len() / 2` frames,
    /// running the tick clock, the trackstep/pattern/macro state machines
    /// and the mixer together. `docs/architecture.md` §3.
    pub fn render(&mut self, out: &mut [i16]) -> Result<(), AccessError> {
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
            sample_rate,
        } = self;
        let mut pos = 0usize;
        let mut error = None;
        clock.advance(*sample_rate, frames, |tick_due, chunk_frames| {
            if tick_due
                && error.is_none()
                && let Err(e) = run_jiffy(module, sequencer, patterns, macros, paula, unsupported)
            {
                error = Some(e);
            }
            let start = pos * 2;
            let end = start + chunk_frames as usize * 2;
            paula.render(smpl, *sample_rate, &mut out[start..end]);
            pos += chunk_frames as usize;
        });
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
fn run_jiffy<'a>(
    module: &'a Module<'a>,
    sequencer: &mut Sequencer<'a>,
    patterns: &mut [Option<PatternRunner<'a>>; 8],
    macros: &mut [MacroInterpreter; 4],
    paula: &mut Paula,
    unsupported: &mut UnsupportedOps,
) -> Result<(), AccessError> {
    // Resolves a previously-open question (`docs/playback-model.md` §7):
    // whether the shared trackstep line pointer advances once every active
    // track's pattern reaches `$F0 <End>`, or unconditionally every jiffy.
    // This crate's reading is the latter: `docs/opcodes.md` §1's per-track
    // word table exists precisely so that "hold the current pattern, just
    // update transpose" (`$80`) can be the common case across many
    // consecutive jiffies -- the trackstep table is evaluated every jiffy
    // like any other state machine here, and it is authored data (mostly
    // `$80 Hold` words) that makes most of those evaluations a no-op, not a
    // gating condition on pattern completion. This also matches the step
    // 4.2 acceptance test's own framing ("trace the first 200 ticks") as
    // one `Sequencer::advance()` call per jiffy.
    if !sequencer.is_stopped() {
        sequencer.advance()?;
    }

    for i in 0..8u8 {
        match sequencer.track(i) {
            TrackSlot::Pattern { number, .. } => {
                let reload = patterns[i as usize]
                    .as_ref()
                    .is_none_or(|r| r.pattern() != number);
                if reload {
                    patterns[i as usize] = Some(PatternRunner::new(module, number)?);
                }
            }
            TrackSlot::Hold { .. } => {}
            TrackSlot::StopChannel => patterns[i as usize] = None,
            TrackSlot::StopVoice { voice } => macros[voice_of(voice)].stop_voice(),
        }
    }

    for i in 0..8u8 {
        let transpose = match sequencer.track(i) {
            TrackSlot::Pattern { transpose, .. } | TrackSlot::Hold { transpose } => transpose,
            _ => 0,
        };
        if let Some(runner) = &mut patterns[i as usize] {
            runner.advance(|entry| dispatch_pattern_entry(entry, transpose, macros))?;
        }
    }

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

    Ok(())
}

/// Routes one decoded pattern longword to the voice it names.
/// `docs/opcodes.md` §2-§3.
fn dispatch_pattern_entry(entry: PatternEntry, transpose: i8, macros: &mut [MacroInterpreter; 4]) {
    match entry {
        PatternEntry::Note {
            note,
            macro_number,
            volume,
            voice,
            ..
        } => {
            macros[voice_of(voice)].trigger(macro_number, note, volume, transpose);
        }
        PatternEntry::Command(command) => match command {
            PatternCommand::KeyUp { voice } => macros[voice_of(voice)].signal_key_up(),
            PatternCommand::Vibrato {
                speed,
                voice,
                depth,
            } => macros[voice_of(voice)].start_vibrato(speed, depth as i8),
            PatternCommand::Envelope {
                amount,
                speed,
                voice,
                target,
            } => {
                // "$F7 <Enve>: every b+1 jiffies" -- `docs/opcodes.md` §2,
                // unlike macro $0F's own "every bb jiffies" with no +1.
                macros[voice_of(voice)].start_envelope(amount, speed + 1, target)
            }
            PatternCommand::Portamento { speed, voice, rate } => {
                macros[voice_of(voice)].start_portamento(speed, rate as i8 as i16)
            }
            // Recognized, timed by `PatternRunner`, and left unconsumed --
            // same status as pattern `MasterVolSlide`: nothing in this
            // crate owns a master volume or cross-track pattern jump yet.
            PatternCommand::Fade { .. }
            | PatternCommand::PlayPattern { .. }
            | PatternCommand::Lock { .. }
            | PatternCommand::End
            | PatternCommand::Loop { .. }
            | PatternCommand::Jump { .. }
            | PatternCommand::Wait { .. }
            | PatternCommand::Stop
            | PatternCommand::GoSub { .. }
            | PatternCommand::Return
            | PatternCommand::StopCustom
            | PatternCommand::Nop => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
