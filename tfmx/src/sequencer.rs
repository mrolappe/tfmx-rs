//! Tick scheduling — the jiffy clock. See `docs/architecture.md` §2: tempo
//! (the tick-rate fraction) and phase (the sub-sample accumulator) are owned
//! here, apart from the mixer, which is what makes block-size independence a
//! property of this type rather than of any single `render()` call.

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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(offsets.iter().enumerate().all(|(i, &o)| o as usize == i * 960));
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
        assert!(offsets
            .iter()
            .enumerate()
            .all(|(i, &o)| u64::from(o) == (i as u64 * 44100 * 60) / (140 * 24)));
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
                assert_eq!(one_call, many, "tempo {tempo} @ {sample_rate} chunk {chunk}");
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
}
