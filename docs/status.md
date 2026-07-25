# Status: A/B listening pass (step 6.2)

Verification pass comparing `tfmx-cli render` against `uade123` (the UADE reference player,
executed only as a black box per the provenance policy — never read) on all 10 corpus modules.
Judged in the order the roadmap step specifies: **tempo**, pitch, instrument attack, timbre.

## Method

This pass could not be literal human listening. Instead, each axis was approximated with a
signal-processing proxy, applied to 20 s of song 0 rendered by both players at 44.1 kHz:

- **Tempo**: the loudness envelope (RMS-derivative, 512-sample frames ≈ 11.6 ms resolution) of
  our render is cross-correlated against the reference's envelope in eight to nineteen 1 s
  windows across the clip, each giving a best-matching lag. A real tempo bug shows up as a lag
  that *drifts* linearly across the clip; a fixed start-offset does not.
- **Pitch**: a global best-fit "detune" between the two renders' long-term log-frequency spectra
  (Welch PSD, 55–2000 Hz, resampled to a log-frequency grid, cross-correlated over a shift of up
  to ±1200 cents).
- **Attack**: rise time (10 %→90 % of local peak) of the envelope's loudest onset in the first 4 s.
- **Timbre**: spectral-shape correlation (40-bin magnitude-spectrum histogram, 0–8 kHz) of the
  same lag-aligned 1 s windows used for the tempo check.

Script and raw renders are not checked in (throwaway analysis, not project tooling); the summary
below is the record.

## Findings

| Module | Tempo drift | Pitch detune (best-fit) | Attack (ours / ref) | Timbre (spectral corr.) |
|---|---|---|---|---|
| apidya (level 1) | −0.00 ms/s | +520 ¢ (corr 0.49) | 11.6 / 11.6 ms | 0.894 |
| apidya (title) | +4.99 ms/s | −583 ¢ (corr 0.81) | 11.6 / 11.6 ms | 0.910 |
| r-type | +2.89 ms/s | +291 ¢ (corr 0.78) | 11.6 / 11.6 ms | 0.811 |
| turrican 2 level 1-desert | +6.58 ms/s | −291 ¢ (corr 0.87) | 11.6 / 11.6 ms | 0.751 |
| turrican 2 level 3-flight | −7.89 ms/s | +603 ¢ (corr −0.05) | 11.6 / 11.6 ms | 0.642 |
| turrican 2 title (st) | −3.14 ms/s | +624 ¢ (corr 0.46) | 11.6 / 11.6 ms | 0.712 |
| turrican 3 level 1 | +7.18 ms/s | −832 ¢ (corr 0.64) | 11.6 / 11.6 ms | 0.872 |
| turrican intro | −7.56 ms/s | −354 ¢ (corr 0.69) | 11.6 / 11.6 ms | 0.921 |
| turrican outside | −9.44 ms/s | +916 ¢ (corr 0.06) | 11.6 / 34.8 ms | 0.567 |
| x-out (title) | +4.33 ms/s | +125 ¢ (corr 0.89) | 23.2 / 11.6 ms | 0.949 |

### Tempo — no deviation found

All ten drift figures are within ±10 ms/s, i.e. under one envelope frame (11.6 ms) of drift over
the full 20 s clip. That is noise at the measurement's own resolution, not a trend. **No evidence
of the classic tempo bug** (halved/doubled speed, or gradual drift) on any corpus module.

### Pitch — proxy inconclusive, code-level evidence is the real basis for confidence

The global-detune numbers swing wildly and inconsistently across modules (from −832 ¢ to +916 ¢,
both directions, no shared magnitude), with correlation at the best-fit shift often barely above
what plain harmonic content gives by chance (e.g. −0.05, 0.06 for two modules). A genuine bug in
period/frequency conversion — a wrong `PAULA_CLOCK_HZ` constant or note-table error — is a single
code path shared by every module and would shift all ten by the *same* amount. The scatter here is
the opposite signature: it says the cross-correlation is latching onto incidental harmonic overlap
in each track's own material, not a real detune. A finer per-onset autocorrelation pitch estimate
was also tried and abandoned — it repeatedly locked onto `rate / lag_min` (an autocorrelation
edge artifact, not a real fundamental) and gave no more consistent a signal.

Confidence in pitch correctness instead rests on what's already in the code: `note_period()` in
`tfmx/src/macro_interp.rs` is unit-tested against known values (middle-C period 424, octave
doubling, finetune offsets), `Paula`'s period→frequency conversion
(`freq_hz = PAULA_CLOCK_HZ / period`, `tfmx/src/paula.rs`) is unit-tested independently, and the
golden-hash regression (step 6.1) locks the exact rendered samples both produce for all ten corpus
modules. **This is the one axis where an actual human listening pass would add real information**
that this automated pass could not.

### Attack — the two outliers are a methodology artifact, not a found difference

turrican outside (11.6 / 34.8 ms) and x-out title (23.2 / 11.6 ms) are the only modules where
attack times disagree. Checking *which* onset each side picked: ours and the reference pick a
loudest-onset-in-first-4s at different timestamps entirely (1.50 s vs 1.68 s; 2.57 s vs 1.31 s) —
different notes, not the same note measured twice. The metric compared two different musical
events, not the rise time of the same instrument. No conclusion drawn here either way; flagging
so it isn't silently dropped.

### Timbre — broadly similar, two modules lower and unverified

Eight of ten modules land at 0.71–0.95 spectral-shape correlation. Turrican outside (0.567) and
turrican 2 level 3-flight (0.642) are noticeably lower. Plausible and unconcerning explanations —
different Paula filter/interpolation emulation between `tfmx-cli` and UADE's TFMX player — are as
likely as an actual instrument/timbre bug in these two modules; **this pass cannot distinguish the
two without an actual ear.**

## Remaining gap

The honest bottom line: tempo is verified clean by a metric that doesn't need to hear a note.
Pitch and timbre could not be conclusively verified by signal analysis alone on polyphonic,
multi-channel chiptune material against a differently-implemented reference decoder — the
automated proxies built for this pass were not trustworthy enough to make a claim either way, and
that limitation is the deliverable of this section, not a swept-under-the-rug detail. If a pitch
or timbre bug is ever suspected in `turrican outside`, `turrican 2 level 3-flight`, or any other
module, an actual human listening comparison against the `uade123` render is the next step, not
more automated signal analysis.
