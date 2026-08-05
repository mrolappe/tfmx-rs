//! Renders a macro or a single pattern standalone to PCM -- no `Sequencer`,
//! no trackstep/pattern layer for the macro case, no track transpose
//! refresh for the pattern case. Mirrors `Player::render_inner`'s
//! tick-then-mix loop (`tfmx/src/player.rs`) at single-voice and
//! single-pattern scale, using the same seams `MacroInterpreter` and
//! `PatternRunner`'s own unit tests already drive standalone. Ported from
//! `tfmx-cli`'s `run_render_macro`/`run_render_pattern` (`docs/gui-plan.md`
//! Phases G2/G3) so a future GUI can render without going through
//! `tfmx-cli`.
//!
//! Unlike the originals, which streamed to `hound` in 4096-frame chunks,
//! these render the whole buffer in one allocation and one
//! [`TickClock::advance`] call: chunking existed only to bound the
//! intermediate buffer handed to the streaming WAV writer, which no longer
//! exists on this side of the extraction.

use tfmx::{
    AccessError, MacroInterpreter, Module, NoteTiming, Paula, PatternCommand, PatternEntry,
    PatternRunner, TickClock, UnsupportedOps,
};

/// Renders `macro_number` triggered with `note`/`volume` on `voice` for
/// `total_frames` stereo frames at `rate`, returning interleaved `i16`
/// samples (`total_frames * 2` long).
#[allow(clippy::too_many_arguments)]
pub fn render_macro_pcm(
    module: &Module,
    macro_number: u8,
    note: u8,
    volume: u8,
    voice: u8,
    tempo: u16,
    rate: u32,
    separation: u8,
    total_frames: usize,
) -> Result<Vec<i16>, AccessError> {
    let mut interp = MacroInterpreter::new();
    let mut paula = Paula::new(separation);
    let mut unsupported = UnsupportedOps::default();
    let mut clock = TickClock::new(tempo);
    let voice = voice & 0x03;
    interp.trigger(macro_number, note, volume, 0);

    let mut pcm = vec![0i16; total_frames * 2];
    let mut pos = 0usize;
    let mut error = None;
    clock.advance(rate, total_frames as u32, |tick_due, span_frames| {
        if tick_due
            && error.is_none()
            && let Err(e) = interp.tick(module, &mut paula, voice, &mut unsupported, |_| {})
        {
            error = Some(e);
        }
        let start = pos * 2;
        let end = start + span_frames as usize * 2;
        paula.render(module.smpl(), rate, &mut pcm[start..end]);
        pos += span_frames as usize;
    });
    if let Some(e) = error {
        return Err(e);
    }
    Ok(pcm)
}

/// Routes one decoded pattern entry to the voice it names -- the same
/// dispatch `Player`'s private `dispatch_pattern_entry` (`tfmx/src/
/// player.rs`) does, reimplemented here against `MacroInterpreter`'s public
/// methods since that function isn't exported. `$FB <PPat>`'s `track`
/// operand is dropped: with only one pattern running there is no second
/// track to jump to, so it's read as "replace the running pattern",
/// covering the common self-loop/chain case but not a real multi-track jump.
fn dispatch_pattern_entry_standalone(
    entry: PatternEntry,
    transpose: i8,
    macros: &mut [MacroInterpreter; 4],
    paula: &mut Paula,
    lock: &mut [u32; 4],
) -> Option<u8> {
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
/// multi-track transpose refresh (`transpose` stands in, constant for the
/// whole render). Mirrors `run_jiffy`'s per-jiffy order (pattern step, then
/// macro tick) at single-pattern scale, the same way [`render_macro_pcm`]
/// mirrors it at single-voice scale.
#[allow(clippy::too_many_arguments)]
pub fn render_pattern_pcm(
    module: &Module,
    pattern: u8,
    transpose: i8,
    tempo: u16,
    rate: u32,
    separation: u8,
    total_frames: usize,
) -> Result<Vec<i16>, AccessError> {
    let mut runner = PatternRunner::new(module, pattern)?;
    let mut macros: [MacroInterpreter; 4] = core::array::from_fn(|_| MacroInterpreter::new());
    let mut paula = Paula::new(separation);
    let mut unsupported = UnsupportedOps::default();
    let mut lock = [0u32; 4];
    let mut clock = TickClock::new(tempo);

    let mut pcm = vec![0i16; total_frames * 2];
    let mut pos = 0usize;
    let mut error = None;
    clock.advance(rate, total_frames as u32, |tick_due, span_frames| {
        if tick_due && error.is_none() {
            let mut jump = None;
            let step = runner.advance(|_pattern, _step, entry| {
                if let Some(target) =
                    dispatch_pattern_entry_standalone(entry, transpose, &mut macros, &mut paula, &mut lock)
                {
                    jump = Some(target);
                }
            });
            match step {
                Ok(()) => {}
                Err(e) => error = Some(e),
            }
            if let Some(target) = jump {
                match PatternRunner::new(module, target) {
                    Ok(r) => runner = r,
                    Err(e) => error = Some(e),
                }
            }
            for remaining in &mut lock {
                *remaining = remaining.saturating_sub(1);
            }
            for (voice, mac) in macros.iter_mut().enumerate() {
                if let Err(e) = mac.tick(module, &mut paula, voice as u8, &mut unsupported, |_| {}) {
                    error = Some(e);
                }
            }
        }
        let start = pos * 2;
        let end = start + span_frames as usize * 2;
        paula.render(module.smpl(), rate, &mut pcm[start..end]);
        pos += span_frames as usize;
    });
    if let Some(e) = error {
        return Err(e);
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        fs::read(path).ok()
    }

    fn wav_sha256(pcm: &[i16]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for sample in pcm {
            hasher.update(sample.to_le_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// G0's golden hash (`tfmx-cli/src/main.rs`'s
    /// `render_macro_output_matches_golden_hash`) computed over the WAV's
    /// decoded `i16` samples -- identical to hashing this function's raw
    /// PCM output, since 16-bit PCM WAV is a lossless container.
    #[test]
    fn render_macro_pcm_matches_g0_golden_hash() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid corpus file");

        let rate = 44_100u32;
        let seconds = 2u32;
        let pcm = render_macro_pcm(
            &module,
            28,
            33,
            64,
            0,
            3,
            rate,
            100,
            rate as usize * seconds as usize,
        )
        .expect("render succeeds on a valid corpus file");

        assert_eq!(pcm.len(), rate as usize * seconds as usize * 2);
        assert_eq!(
            wav_sha256(&pcm),
            "17dc48c406be2115179f34f44c8602397b696d9c2f442be8d22328446aa5fc11",
            "render_macro_pcm output changed -- if intentional, update this hash"
        );
    }
}
