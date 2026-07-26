//! `Paula` and `Voice` — the four-voice register file. See
//! `docs/architecture.md` §2 for the register seam this implements: the
//! sequencer writes `Voice` fields through `Paula`'s setters, and
//! `Paula::render()` (step 3.2) reads them.

use crate::macro_interp::Envelope;

const VOICE_COUNT: usize = 4;

/// PAL Paula reference clock. `docs/playback-model.md` §2.1.
const PAULA_CLOCK_HZ: f64 = 3_546_895.0;

/// `frac`'s fixed-point precision: high bits are the whole-sample position
/// within the voice's currently active region, low bits the interpolation
/// weight between that sample and the next.
const FRAC_BITS: u32 = 32;

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
    /// Sub-sample playback position within `[start, start+len)`, as a
    /// `FRAC_BITS`-fraction fixed-point value; mixer-internal only (step 3.2).
    frac: u64,
    /// Times the active sample region has completed a play-through since
    /// the last `reset_loop_completions` call -- the `$1A` feedback path.
    /// `docs/playback-model.md` §6.
    loop_completions: u32,
}

impl Voice {
    /// Reads one interpolated PCM sample and advances playback position by
    /// one output sample at `sample_rate`, handling the one-shot-then-loop
    /// auto-reload (`docs/playback-model.md` §2.3): once the active region
    /// is exhausted, playback switches to `loop_start`/`loop_len` -- exactly
    /// what a well-timed `$18 Sampleloop` rewrite would produce, but done
    /// once here instead of relying on tick-accurate register timing.
    fn next_sample(&mut self, smpl: &[i8], sample_rate: u32) -> f64 {
        if self.period == 0 || sample_rate == 0 {
            return 0.0;
        }
        // docs/format.md §8: Paula's length register is a word count, 1 = 2 bytes.
        let len_samples = self.len as u64 * 2;
        if len_samples == 0 {
            return 0.0;
        }

        let pos = (self.frac >> FRAC_BITS) as usize;
        let weight = (self.frac & (u32::MAX as u64)) as f64 / (1u64 << FRAC_BITS) as f64;
        let base = self.start as usize;
        let s0 = smpl.get(base + pos).copied().unwrap_or(0);
        let s1 = smpl.get(base + pos + 1).copied().unwrap_or(0);
        let value = s0 as f64 * (1.0 - weight) + s1 as f64 * weight;

        let freq_hz = PAULA_CLOCK_HZ / self.period as f64;
        let step = ((freq_hz / sample_rate as f64) * (1u64 << FRAC_BITS) as f64) as u64;
        self.frac = self.frac.wrapping_add(step.max(1));

        if (self.frac >> FRAC_BITS) >= len_samples {
            self.frac -= len_samples << FRAC_BITS;
            if self.start != self.loop_start || self.len != self.loop_len {
                self.start = self.loop_start;
                self.len = self.loop_len;
            }
            self.loop_completions = self.loop_completions.wrapping_add(1);
        }

        value
    }
}

/// Paula's fixed stereo wiring: voices 0 and 3 are hard-panned left, 1 and 2
/// hard-panned right. **[HW]**, standard Amiga hardware, not TFMX-specific.
fn is_left_voice(voice: usize) -> bool {
    matches!(voice, 0 | 3)
}

/// `separation` (0-100, percent): how much of a voice's signal reaches its
/// *own* channel vs. bleeds into the opposite one. 100 = hardware-accurate
/// hard pan (no bleed); 0 = both channels get the full mix. This knob and
/// its 0-100 scale are this crate's own convention -- neither [S1] nor the
/// other sources document stereo behavior at all.
fn pan_weights(separation: u8) -> (f64, f64) {
    let bleed = 1.0 - (separation.min(100) as f64 / 100.0);
    (1.0, bleed)
}

/// The four-voice register file the sequencer writes and the mixer
/// (step 3.2) reads. `docs/architecture.md` §2.
#[derive(Debug)]
pub struct Paula {
    voices: [Voice; VOICE_COUNT],
    separation: u8,
    muted: [bool; VOICE_COUNT],
    /// 0..=64, mixed in as an overall attenuation alongside each voice's own
    /// `volume`. Defaults to 64 (no attenuation) -- a bare mixer with no
    /// slide ever started has no reason to silence itself. The song-start
    /// policy of defaulting to 0 (`docs/status.md`'s "Update (2026-07-26,
    /// later)" section) belongs to whoever knows it's song start, not here;
    /// see `Player::new`.
    master_volume: u8,
    master_volume_slide: Option<Envelope>,
}

impl Paula {
    /// `separation`: hardware-panning knob consumed by `render()` (step 3.2).
    pub fn new(separation: u8) -> Self {
        Paula {
            voices: [Voice::default(); VOICE_COUNT],
            separation,
            muted: [false; VOICE_COUNT],
            master_volume: 64,
            master_volume_slide: None,
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

    /// Turning DMA on latches a fresh fetch at the currently-set start
    /// (`docs/playback-model.md` §2.3: "`$01 DMAon` starts it"): an off→on
    /// transition resets the sub-sample position so a retrigger begins at
    /// the new region's first sample instead of resuming wherever the
    /// previous region's playback happened to leave off. An on→on call (or
    /// a `$18 Sampleloop` rewrite while DMA stays continuously on) must NOT
    /// reset it -- that's the timed attack→loop handoff `Voice::next_sample`
    /// already performs on its own.
    pub fn set_dma(&mut self, voice: u8, on: bool) {
        let v = &mut self.voices[voice as usize];
        if on && !v.dma_on {
            v.frac = 0;
        }
        v.dma_on = on;
    }

    /// Times `voice`'s active sample region has completed a play-through
    /// since the last `reset_loop_completions` call -- the `$1A <Wait on
    /// DMA>` feedback path. `docs/playback-model.md` §6.
    pub fn loop_completions(&self, voice: u8) -> u32 {
        self.voices[voice as usize].loop_completions
    }

    pub fn reset_loop_completions(&mut self, voice: u8) {
        self.voices[voice as usize].loop_completions = 0;
    }

    /// Mutes `voice` at the mix: `render()` still runs its `next_sample`
    /// unconditionally, so playback position, loop completions and the
    /// attack→loop handoff evolve identically muted or not -- only the
    /// contribution to the output buffer is dropped. This is what makes a
    /// muted voice's stem subtract cleanly out of the full mix.
    pub fn set_voice_muted(&mut self, voice: u8, muted: bool) {
        self.muted[voice as usize] = muted;
    }

    /// A copy of `voice`'s register state, for diagnostics (the trace seam,
    /// step 11.3) and tests.
    pub fn voice(&self, voice: u8) -> Voice {
        self.voices[voice as usize]
    }

    /// Starts (or replaces) a master-volume slide: every `every` jiffies,
    /// move by 1 towards `target`, clamping on arrival. Shared mechanic for
    /// pattern `$FA <Fade>` and trackstep `$EFFE 0003`/`0004`.
    /// `docs/playback-model.md` §5.1.
    pub fn start_master_volume_slide(&mut self, every: u8, target: u8) {
        self.master_volume_slide = Some(Envelope::new(1, every, target));
    }

    /// Advances one jiffy; a no-op once no slide is active or it has
    /// reached its target.
    pub fn tick_master_volume(&mut self) {
        if let Some(envelope) = &mut self.master_volume_slide
            && !envelope.tick(&mut self.master_volume)
        {
            self.master_volume_slide = None;
        }
    }

    /// The current master-volume value (0..=64).
    #[cfg(test)]
    pub(crate) fn master_volume(&self) -> u8 {
        self.master_volume
    }

    /// Sets the master volume directly, bypassing any slide. Used by
    /// `Player::new` to apply the song-start default.
    pub fn set_master_volume(&mut self, value: u8) {
        self.master_volume = value;
    }

    /// Synthesizes `out.len() / 2` interleaved stereo frames from whatever
    /// `Voice` state is currently latched, reading PCM out of `smpl`.
    /// Register state is constant across the call. `docs/architecture.md` §3.
    pub fn render(&mut self, smpl: &[i8], sample_rate: u32, out: &mut [i16]) {
        let (own, bleed) = pan_weights(self.separation);
        let muted = self.muted;
        let master = self.master_volume as f64 / 64.0;
        for frame in out.chunks_exact_mut(2) {
            let mut left = 0.0_f64;
            let mut right = 0.0_f64;
            for (i, voice) in self.voices.iter_mut().enumerate() {
                if !voice.dma_on {
                    continue;
                }
                // 8-bit PCM volume-scaled and expanded into i16 range.
                let amp = voice.next_sample(smpl, sample_rate)
                    * (voice.volume as f64 / 64.0)
                    * master
                    * 256.0;
                if muted[i] {
                    continue;
                }
                let (own_ch, other_ch) = if is_left_voice(i) {
                    (&mut left, &mut right)
                } else {
                    (&mut right, &mut left)
                };
                *own_ch += amp * own;
                *other_ch += amp * bleed;
            }
            frame[0] = left.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            frame[1] = right.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
        }
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
    fn new_defaults_master_volume_to_full_scale() {
        // A bare mixer with no slide ever started must not silently
        // attenuate -- that policy belongs to whoever knows it's song
        // start (`Player::new`), not to `Paula` itself.
        let paula = Paula::new(0);
        assert_eq!(paula.master_volume(), 64);
    }

    #[test]
    fn master_volume_slide_moves_by_one_every_divisor_jiffies_and_clamps() {
        let mut paula = Paula::new(0);
        paula.set_master_volume(0);
        paula.start_master_volume_slide(2, 3);

        paula.tick_master_volume();
        assert_eq!(paula.master_volume(), 0, "first tick just starts the counter");
        paula.tick_master_volume();
        assert_eq!(paula.master_volume(), 1);
        paula.tick_master_volume();
        paula.tick_master_volume();
        assert_eq!(paula.master_volume(), 2);
        paula.tick_master_volume();
        paula.tick_master_volume();
        assert_eq!(paula.master_volume(), 3);
        paula.tick_master_volume();
        paula.tick_master_volume();
        assert_eq!(paula.master_volume(), 3, "clamped at target, stops advancing");
    }

    #[test]
    fn render_is_silent_at_zero_master_volume_even_with_a_voice_at_full_volume() {
        let mut paula = Paula::new(100);
        paula.set_master_volume(0);
        let smpl = [100i8; 16];
        paula.set_period(0, 100);
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 8);
        paula.set_loop_region(0, 0, 8);
        paula.set_dma(0, true);

        let mut out = [1i16; 8];
        paula.render(&smpl, 48_000, &mut out);

        assert_eq!(out, [0i16; 8]);
    }

    #[test]
    fn render_scales_linearly_with_master_volume() {
        let smpl = [100i8; 16];
        let make = |master: u8| {
            let mut p = Paula::new(100);
            p.set_master_volume(master);
            p.set_period(0, 100);
            p.set_volume(0, 64);
            p.set_sample_region(0, 0, 8);
            p.set_loop_region(0, 0, 8);
            p.set_dma(0, true);
            p
        };

        let mut half = make(32);
        let mut out_half = [0i16; 8];
        half.render(&smpl, 48_000, &mut out_half);

        let mut full = make(64);
        let mut out_full = [0i16; 8];
        full.render(&smpl, 48_000, &mut out_full);

        assert!(out_full[0] > 0, "full master volume must not be silent");
        let ratio = out_half[0] as f64 / out_full[0] as f64;
        assert!(
            (ratio - 0.5).abs() < 0.02,
            "half master volume should render at ~half amplitude, got ratio {ratio}"
        );
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

    #[test]
    fn render_with_all_dma_off_is_silent() {
        let mut paula = Paula::new(100);
        let smpl = [0i8; 16];
        let mut out = [1i16; 8]; // pre-filled with non-zero to prove render() writes it

        paula.render(&smpl, 48_000, &mut out);

        assert_eq!(out, [0i16; 8]);
    }

    // docs/playback-model.md §2.1: freq_hz = 3_546_895 / period. Independent
    // of Paula::render()'s own interpolation code -- this is the textbook
    // constant, not a value read back out of the implementation.
    #[test]
    fn render_reproduces_known_pitch_by_zero_crossing_count() {
        // Low enough period that source_rate_hz sits well above signal_hz --
        // otherwise the fixture itself violates Nyquist before Paula ever
        // sees it.
        let period: u16 = 80;
        let source_rate_hz = PAULA_CLOCK_HZ / period as f64;
        let signal_hz = 1000.0;
        let sample_rate = 48_000u32;
        let duration_s = 1.0;

        // A signal_hz sine, stored as if sampled at source_rate_hz -- the
        // rate Paula will fetch it at, so no resampling error is baked into
        // the fixture itself.
        let source_len = (source_rate_hz * duration_s).ceil() as usize + 4;
        let source: Vec<i8> = (0..source_len)
            .map(|i| {
                let t = i as f64 / source_rate_hz;
                (127.0 * (2.0 * std::f64::consts::PI * signal_hz * t).sin()).round() as i8
            })
            .collect();

        let mut paula = Paula::new(100);
        paula.set_period(0, period);
        paula.set_volume(0, 64);
        let len_words = (source_len as u32).div_ceil(2);
        paula.set_sample_region(0, 0, len_words);
        paula.set_loop_region(0, 0, len_words); // same as attack: seamless repeat if exhausted
        paula.set_dma(0, true);

        let frames = (sample_rate as f64 * duration_s) as usize;
        let mut out = vec![0i16; frames * 2];
        paula.render(&source, sample_rate, &mut out);

        // Voice 0 is hard-panned left (docs/architecture.md's Paula channel
        // wiring, 0&3 = left, 1&2 = right -- standard Amiga hardware, see
        // pan_weights below).
        let mut crossings = 0u32;
        let mut prev_sign = 0i32;
        for frame in out.chunks_exact(2) {
            let sign = frame[0].signum() as i32;
            if sign != 0 {
                if prev_sign != 0 && sign != prev_sign {
                    crossings += 1;
                }
                prev_sign = sign;
            }
        }

        let expected = 2.0 * signal_hz * duration_s;
        let tolerance = expected * 0.005;
        assert!(
            (crossings as f64 - expected).abs() <= tolerance,
            "expected ~{expected} zero crossings (±0.5%), got {crossings}"
        );
    }

    #[test]
    fn render_transitions_from_attack_to_loop_region() {
        let sample_rate = 8_000u32;
        // ~one source sample consumed per output frame.
        let period = (PAULA_CLOCK_HZ / sample_rate as f64).round() as u16;

        let mut source = vec![0i8; 200];
        source[0..100].fill(100);
        source[100..200].fill(-100);

        let mut paula = Paula::new(100);
        paula.set_period(0, period);
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 50); // 50 words = 100 samples: the attack region
        paula.set_loop_region(0, 100, 50); // 100 samples starting at byte 100
        paula.set_dma(0, true);

        let mut out = vec![0i16; 250 * 2];
        paula.render(&source, sample_rate, &mut out);

        let left: Vec<i16> = out.iter().step_by(2).copied().collect();
        assert!(
            left[10] > 0,
            "expected attack-region output, got {}",
            left[10]
        );
        assert!(
            left[150] < 0,
            "expected loop-region output after transition, got {}",
            left[150]
        );
    }

    #[test]
    fn dma_retrigger_resets_sub_sample_position() {
        let sample_rate = 8_000u32;
        // ~one source sample consumed per output frame.
        let period = (PAULA_CLOCK_HZ / sample_rate as f64).round() as u16;

        // Region B [100,150): an index-coded ramp so byte k there reads back
        // as -100 + k -- a leftover mid-region position carried over from
        // region A would surface as a too-high value instead of the correct
        // first sample, -100.
        let mut source = vec![50i8; 200];
        for k in 0..50i32 {
            source[100 + k as usize] = (-100 + k) as i8;
        }

        let mut paula = Paula::new(100);
        paula.set_period(0, period);
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 50); // region A: 50 words = 100 samples
        paula.set_loop_region(0, 0, 50);
        paula.set_dma(0, true);

        // Consume a non-multiple of the region length, so playback position
        // sits mid-region (~37 samples in) rather than conveniently at 0.
        let mut out = vec![0i16; 137 * 2];
        paula.render(&source, sample_rate, &mut out);

        // A retrigger, exactly as `$00 DMAoff` + `$02/$03 SetBegin/SetLen` +
        // `$01 DMAon` produce: DMA off, a brand-new region, DMA on.
        paula.set_dma(0, false);
        paula.set_sample_region(0, 100, 25); // region B: 25 words = 50 samples
        paula.set_loop_region(0, 100, 25);
        paula.set_dma(0, true);

        let mut out2 = vec![0i16; 2];
        paula.render(&source, sample_rate, &mut out2);
        let amp = out2[0] as f64 / 256.0; // undo the volume=64 (x1.0) * 256 scaling
        assert!(
            (amp - -100.0).abs() < 1.0,
            "expected playback to restart at region B's first sample (~-100), got {amp}"
        );
    }

    #[test]
    fn render_volume_zero_is_silent() {
        let sample_rate = 8_000u32;
        let period = 443u16;
        let source = vec![100i8; 50];

        let mut paula = Paula::new(100);
        paula.set_period(0, period);
        paula.set_volume(0, 0);
        paula.set_sample_region(0, 0, 25); // 25 words = 50 samples
        paula.set_loop_region(0, 0, 25);
        paula.set_dma(0, true);

        let mut out = vec![1i16; 40 * 2]; // pre-filled non-zero
        paula.render(&source, sample_rate, &mut out);

        assert!(
            out.iter().all(|&s| s == 0),
            "expected silence at volume 0, got {out:?}"
        );
    }

    #[test]
    fn render_clamps_combined_output_to_i16_range() {
        let sample_rate = 8_000u32;
        let period = 443u16;
        let source = vec![127i8; 50]; // max positive amplitude

        let mut paula = Paula::new(0); // separation 0: every channel gets the full mix
        for voice in 0..4u8 {
            paula.set_period(voice, period);
            paula.set_volume(voice, 64);
            paula.set_sample_region(voice, 0, 25);
            paula.set_loop_region(voice, 0, 25);
            paula.set_dma(voice, true);
        }

        let mut out = vec![0i16; 10 * 2];
        paula.render(&source, sample_rate, &mut out);

        assert!(
            out.iter().all(|&s| s == i16::MAX),
            "expected four full-scale voices summed and clamped to i16::MAX, got {out:?}"
        );
    }

    #[test]
    fn loop_completions_counts_wraparounds() {
        let sample_rate = 8_000u32;
        // one source sample consumed per output frame.
        let period = (PAULA_CLOCK_HZ / sample_rate as f64).round() as u16;
        let source = vec![100i8; 100]; // 50-word loop region

        let mut paula = Paula::new(0);
        paula.set_period(0, period);
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 50);
        paula.set_loop_region(0, 0, 50);
        paula.set_dma(0, true);

        assert_eq!(paula.loop_completions(0), 0);

        let mut out = vec![0i16; 1000 * 2]; // 10 loop lengths
        paula.render(&source, sample_rate, &mut out);

        assert_eq!(paula.loop_completions(0), 10);
    }

    #[test]
    fn reset_loop_completions_zeroes_only_target_voice() {
        let sample_rate = 8_000u32;
        let period = (PAULA_CLOCK_HZ / sample_rate as f64).round() as u16;
        let source = vec![100i8; 100];

        let mut paula = Paula::new(0);
        for voice in 0..2u8 {
            paula.set_period(voice, period);
            paula.set_volume(voice, 64);
            paula.set_sample_region(voice, 0, 50);
            paula.set_loop_region(voice, 0, 50);
            paula.set_dma(voice, true);
        }

        let mut out = vec![0i16; 500 * 2]; // 5 loop lengths
        paula.render(&source, sample_rate, &mut out);
        assert_eq!(paula.loop_completions(0), 5);
        assert_eq!(paula.loop_completions(1), 5);

        paula.reset_loop_completions(0);
        assert_eq!(paula.loop_completions(0), 0);
        assert_eq!(paula.loop_completions(1), 5);
    }

    #[test]
    fn set_voice_muted_updates_only_target_voice() {
        let mut paula = Paula::new(0);
        paula.set_voice_muted(1, true);
        assert!(paula.muted[1]);
        assert!(!paula.muted[0]);
    }

    #[test]
    fn render_with_voice_muted_is_silent() {
        let sample_rate = 8_000u32;
        let period = 443u16;
        let source = vec![50i8; 50];

        let mut paula = Paula::new(100);
        paula.set_period(0, period);
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 25);
        paula.set_loop_region(0, 0, 25);
        paula.set_dma(0, true);
        paula.set_voice_muted(0, true);

        let mut out = vec![1i16; 20 * 2]; // pre-filled non-zero
        paula.render(&source, sample_rate, &mut out);

        assert!(
            out.iter().all(|&s| s == 0),
            "expected silence from a muted voice, got {out:?}"
        );
    }

    #[test]
    fn muted_voice_playback_position_still_advances() {
        let sample_rate = 8_000u32;
        let period = (PAULA_CLOCK_HZ / sample_rate as f64).round() as u16;
        let source = vec![50i8; 50];

        let mut paula = Paula::new(100);
        paula.set_period(0, period);
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 25);
        paula.set_loop_region(0, 0, 25);
        paula.set_dma(0, true);
        paula.set_voice_muted(0, true);

        assert_eq!(paula.voice(0).frac, 0);

        let mut out = vec![0i16; 10 * 2];
        paula.render(&source, sample_rate, &mut out);

        assert_ne!(
            paula.voice(0).frac,
            0,
            "a muted voice's playback position must still advance"
        );
    }

    #[test]
    fn solo_renders_of_all_four_voices_sum_to_the_full_mix() {
        let sample_rate = 8_000u32;
        let period = 443u16;
        // Flat source: every interpolated sample reads back as the same
        // value regardless of sub-sample position, so the test isn't
        // sensitive to frac drift between the full render and the solos.
        let source = vec![50i8; 50];
        let frames = 20;

        let configure = |paula: &mut Paula| {
            for voice in 0..4u8 {
                paula.set_period(voice, period);
                paula.set_volume(voice, 16); // low enough that the sum of all four can't clip
                paula.set_sample_region(voice, 0, 25);
                paula.set_loop_region(voice, 0, 25);
                paula.set_dma(voice, true);
            }
        };

        let mut full = Paula::new(100);
        configure(&mut full);
        let mut full_out = vec![0i16; frames * 2];
        full.render(&source, sample_rate, &mut full_out);

        let mut summed = vec![0i32; frames * 2];
        for solo in 0..4u8 {
            let mut paula = Paula::new(100);
            configure(&mut paula);
            for voice in 0..4u8 {
                paula.set_voice_muted(voice, voice != solo);
            }
            let mut out = vec![0i16; frames * 2];
            paula.render(&source, sample_rate, &mut out);
            for (acc, s) in summed.iter_mut().zip(out.iter()) {
                *acc += *s as i32;
            }
        }

        let full_i32: Vec<i32> = full_out.iter().map(|&s| s as i32).collect();
        assert_eq!(
            summed, full_i32,
            "the four solo renders must sum sample-for-sample to the full mix"
        );
    }

    #[test]
    fn render_separation_100_hard_pans_voices() {
        let sample_rate = 8_000u32;
        let period = 443u16;
        let source = vec![100i8; 50];

        let mut paula = Paula::new(100); // full hard pan, no bleed
        paula.set_period(0, period); // voice 0 is hard-panned left
        paula.set_volume(0, 64);
        paula.set_sample_region(0, 0, 25);
        paula.set_loop_region(0, 0, 25);
        paula.set_dma(0, true);

        let mut out = vec![0i16; 10 * 2];
        paula.render(&source, sample_rate, &mut out);

        let left: Vec<i16> = out.iter().step_by(2).copied().collect();
        let right: Vec<i16> = out.iter().skip(1).step_by(2).copied().collect();
        assert!(
            left.iter().any(|&s| s != 0),
            "left channel should carry voice 0's signal"
        );
        assert!(
            right.iter().all(|&s| s == 0),
            "separation 100 should leave the opposite channel silent, got {right:?}"
        );
    }
}
