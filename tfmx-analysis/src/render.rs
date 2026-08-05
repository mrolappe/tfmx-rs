//! Renders a macro standalone to PCM -- no `Sequencer`, no trackstep/pattern
//! layer, no track transpose. Mirrors `Player::render_inner`'s tick-then-mix
//! loop (`tfmx/src/player.rs`) at a single-voice scale, using the same seam
//! `MacroInterpreter`'s own unit tests already drive standalone. Ported from
//! `tfmx-cli`'s `run_render_macro` (`docs/gui-plan.md` Phase G2) so a future
//! GUI can render without going through `tfmx-cli`.
//!
//! Unlike the original, which streamed to `hound` in 4096-frame chunks, this
//! renders the whole buffer in one allocation and one [`TickClock::advance`]
//! call: chunking existed only to bound the intermediate buffer handed to
//! the streaming WAV writer, which no longer exists on this side of the
//! extraction.

use tfmx::{AccessError, MacroInterpreter, Module, Paula, TickClock, UnsupportedOps};

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
