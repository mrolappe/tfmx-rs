//! `Paula` and `Voice` — the four-voice register file. See
//! `docs/architecture.md` §2 for the register seam this implements: the
//! sequencer writes `Voice` fields through `Paula`'s setters, and
//! `Paula::render()` (step 3.2) reads them.

const VOICE_COUNT: usize = 4;

/// One Paula voice's register state, as written by the sequencer and read
/// by the mixer. `docs/architecture.md` §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Voice {
    /// Absolute byte offset into `smpl`. `docs/format.md` §8.
    pub start: u32,
    /// Sample length in words (1 word = 2 bytes). `docs/format.md` §2.
    pub len: u32,
    /// Paula period; `freq_hz = 3_546_895 / period`. `docs/playback-model.md` §2.1.
    pub period: u16,
    /// 0..=64. `docs/playback-model.md` §2.2.
    pub volume: u8,
    pub dma_on: bool,
    pub loop_start: u32,
    pub loop_len: u32,
    /// Sub-sample playback position; mixer-internal only (step 3.2).
    frac: u32,
}

/// The four-voice register file the sequencer writes and the mixer
/// (step 3.2) reads. `docs/architecture.md` §2.
#[derive(Debug)]
pub struct Paula {
    voices: [Voice; VOICE_COUNT],
    separation: u8,
}

impl Paula {
    /// `separation`: hardware-panning knob consumed by `render()` (step 3.2).
    pub fn new(separation: u8) -> Self {
        Paula {
            voices: [Voice::default(); VOICE_COUNT],
            separation,
        }
    }

    pub fn set_period(&mut self, voice: u8, period: u16) {
        self.voices[voice as usize].period = period;
    }

    pub fn set_volume(&mut self, voice: u8, volume: u8) {
        self.voices[voice as usize].volume = volume;
    }

    pub fn set_sample_region(&mut self, voice: u8, start: u32, len: u32) {
        let v = &mut self.voices[voice as usize];
        v.start = start;
        v.len = len;
    }

    pub fn set_loop_region(&mut self, voice: u8, loop_start: u32, loop_len: u32) {
        let v = &mut self.voices[voice as usize];
        v.loop_start = loop_start;
        v.loop_len = loop_len;
    }

    pub fn set_dma(&mut self, voice: u8, on: bool) {
        self.voices[voice as usize].dma_on = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_four_silent_voices() {
        let paula = Paula::new(0);
        assert_eq!(paula.voices, [Voice::default(); 4]);
    }

    #[test]
    fn set_period_updates_only_target_voice() {
        let mut paula = Paula::new(0);
        paula.set_period(1, 428);
        assert_eq!(paula.voices[1].period, 428);
        assert_eq!(paula.voices[0].period, 0);
    }

    #[test]
    fn set_volume_updates_only_target_voice() {
        let mut paula = Paula::new(0);
        paula.set_volume(2, 64);
        assert_eq!(paula.voices[2].volume, 64);
        assert_eq!(paula.voices[0].volume, 0);
    }

    #[test]
    fn set_sample_region_updates_start_and_len() {
        let mut paula = Paula::new(0);
        paula.set_sample_region(0, 0x1000, 200);
        assert_eq!(paula.voices[0].start, 0x1000);
        assert_eq!(paula.voices[0].len, 200);
    }

    #[test]
    fn set_loop_region_updates_loop_start_and_len() {
        let mut paula = Paula::new(0);
        paula.set_loop_region(3, 0x2000, 50);
        assert_eq!(paula.voices[3].loop_start, 0x2000);
        assert_eq!(paula.voices[3].loop_len, 50);
    }

    #[test]
    fn set_dma_updates_only_target_voice() {
        let mut paula = Paula::new(0);
        paula.set_dma(0, true);
        assert!(paula.voices[0].dma_on);
        assert!(!paula.voices[1].dma_on);
    }
}
