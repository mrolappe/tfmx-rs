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

## Open follow-up (2026-07-26, post-6.2): human listening found a real problem this pass missed

An informal human listening pass (not the `uade123` A/B above) on `apidya (title)` and one
`turrican` corpus module found the render does not sound like the source material at all --
`apidya (title)` in particular sounded like one sample fragment looping continuously rather than
music. Investigation (see conversation record, not reproduced here) found and fixed a real bug:
`Paula::Voice`'s sub-sample playback position (`frac`) was never reset on a DMA off-to-on
retrigger, so every note-on resumed mid-sample at a leftover offset instead of starting the new
region's attack from its beginning -- confirmed with a targeted unit test
(`dma_retrigger_resets_sub_sample_position` in `tfmx/src/paula.rs`) and fixed in `Paula::set_dma`.
All ten golden hashes (step 6.1) changed as expected and were regenerated.

**This fix did not produce an audible improvement on a second listen.** The `frac`-reset bug was
real and is correctly fixed, but it is evidently not the (or not the only) cause of what was
heard. Deferred rather than chased further in the same session; the next person picking this up
should not assume the `frac` fix already solved it. Concrete next steps, in the order this
document's own "Remaining gap" section already recommends: an actual `uade123` A/B on `apidya
(title)` specifically (not just the automated proxies above, which only ever covered pitch/timbre
correlation, not "does this sound like a coherent song at all"), and re-running the trackstep/
pattern trace diagnostic from the conversation (distinct patterns per track, notes triggered,
command histogram) against `uade123`'s own known-correct behavior if that becomes observable.

## Update (2026-07-26, step 11.2): `apidya (title)` is TFMX 7V — the wrong format entirely

Before the stems listening pass, `uade123 -g` (info-only, no playback — the file-format probe
this player exposes for free) was run against every corpus module to read back which eagleplayer
it auto-detects:

| module | `uade123` playername |
|---|---|
| apidya (title) | **TFMX 7V** |
| apidya (level 1) | TFMX Pro |
| turrican 2 level 1-desert | TFMX Pro |
| turrican 2 level 3-flight | TFMX Pro |
| turrican 2 title (st) | TFMX Pro |
| turrican 3 level 1 | TFMX Pro |
| r-type | TFMX |
| turrican intro | TFMX |
| turrican outside | TFMX |
| x-out (title) | TFMX |

**`apidya (title)` is the only corpus file `uade123` identifies as TFMX 7V** — the variant
`docs/architecture.md` §9 and `docs/playback-model.md` already document this crate as explicitly
*not* supporting (7V multiplexes four virtual voices per hardware channel; this crate targets
TFMX Professional 2.0 only, per [S1]/[S2]). This is a strong, likely-sufficient explanation for why
`apidya (title)` alone renders as "one sample fragment looping continuously": the parser is reading
7V-encoded data as if it were Pro-2.0, i.e. genuinely out-of-scope input rather than a bug in the
Pro-2.0 implementation. `apidya (level 1)` and both `turrican 2`/`turrican 3` files are "TFMX Pro"
(not plainly "TFMX") in `uade123`'s own naming — unclear yet whether that label tracks a real
sub-format difference within Pro-2.0 or is just `uade123`'s internal player-module naming; not
investigated further this pass.

**Implication for corpus-wide diagnosis (step 11.5's `lint`)**: `apidya (title)`'s result should
be interpreted as "wrong format," not folded into the same bucket as a genuine Pro-2.0 playback
bug. Whether `turrican intro`'s issue (the one other module named in the original informal
listening pass) is a real Pro-2.0 bug remains open — it *is* plain "TFMX" per the table above, so
this format explanation does not apply to it.

### Stems listening (step 11.2), user's own ears, `tfmx-cli render --stems`

`apidya (title)`, song 0, first 5 s:
- v0, v1, v2: each sounds like the same one sample fragment looping continuously.
- v3: completely silent.

Consistent with the TFMX 7V finding above — this is what feeding 7V data through a Pro-2.0 parser
would plausibly produce (voices reading garbage or a stuck-region interpretation of misinterpreted
control data; one voice reading a region that never gets DMA'd on at all).

`turrican intro`, song 0, first 5 s (plain TFMX — the format this crate targets):
- v0: two low/bass notes, then two higher notes, then another low note.
- v1: a single note, after a several-second delay.
- v2: a recognizable melodic line.
- v3: several bass-drum-like hits.

Nothing here reads as obviously broken by ear alone (a sparse intro voice, a melody, a drum voice,
a bass line are all plausible parts of a game-intro arrangement) — this listen alone can't confirm
or rule out a bug; it needs the `uade123` reference below.

### `uade123` full-mix reference now available for direct A/B

`uade123` has no per-voice solo/mute flag (checked `uade123 -h` in full; nothing in `--ep-option`
is documented for it either, and per the provenance policy its source is never read to look for an
undocumented one). But a full-mix reference render is trivial and was not on hand before this
session: `uade123 -s <song> -t <seconds> -f out.wav "mdat.<name>"` (auto-locates the matching
`smpl.<name>`), executed as a black box exactly like the existing step 6.2 A/B pass already does.
Reference renders of `apidya (title)` and `turrican intro` (song 0, 10 s each) were produced this
session to `scratchpad` for a first listen; not checked into the repo (ephemeral, regeneratable by
the command above). The step 6.2 proxies already do this same full-mix comparison, just via
signal-processing metrics instead of ears — this is the "actual `uade123` A/B" this document's
"Remaining gap" section named as the most promising untried step, now finally done for real audio.

### `turrican intro` A/B result: confirmed real, and it is not a subtle bug

The user's listen (song 0, first 10 s) found the two renders **meaningfully different**, not just
off in timing or timbre:

- `uade123` reference: higher pitch overall; a staccato ostinato; a pad playing shifting pitches;
  **no percussion**.
- This crate's render: starts on a different patch/instrument than the reference; **has percussion
  (bass drum)** the reference doesn't; plays a different melody entirely.

This rules out the two most likely "boring" explanations up front: it is not a mixing/pan/volume
issue (the *musical content* differs, not just its balance), and it is not the `apidya`-style
wrong-format explanation (`turrican intro` is confirmed plain TFMX, `uade123`'s own target format).
A different melody, a different starting patch, and an extra percussion voice that shouldn't be
there together point at **wrong pattern/macro/note data being dispatched** — the trackstep or
pattern layer resolving to different content than the reference, not a Paula-level mixing defect.
This is now the corpus's confirmed, still-uninvestigated Pro-2.0 playback bug; `apidya (title)`'s
symptom is separately explained (wrong format, see above) and should not be conflated with this
one when deciding what step 11.5's `lint` findings mean or what to repair at the Phase 11 gate.

**Not yet checked**: whether `uade123`'s song 0 and this crate's song 0 are actually the same
subsong (both were selected by index, not by name — the pointer tables could legitimately number
them differently), and whether the percussion voice is on a different voice number in each render
(the per-voice stems above were never directly played back against a per-voice reference, since
`uade123` has no solo). Both are prerequisites before concluding *where* in the trackstep → pattern
→ macro chain the divergence starts — exactly what step 11.3's trace seam is for.

## Update (2026-07-26, step 11.5): corpus-wide `tfmx-cli lint` results

`tfmx-cli lint <mdat> <smpl> [--song N] [--seconds S]` runs a traced render, folds the
`TraceEvent` stream plus the unsupported-opcode counters plus the rendered PCM into one report,
and names what looks wrong. The table below is the deliverable of step 11.5: **song 0, 30 s,
44.1 kHz, separation 100**, every corpus module, all findings as reported by the tool.

| module | jiffies | tempos | lines | loop/stop | patterns | note-ons v0/v1/v2/v3 | peak | clipped | findings |
|---|---|---|---|---|---|---|---|---|---|
| `turrican intro` | 375 | 3 | 55 | loop/— | 67 | 206/245/129/205 | 32768 | 7280 | no-retrigger v1 (start=36868 len=4480); clipping 0.28% |
| `turrican outside` | 300 | 4 | 38 | loop/— | 29 | 211/194/138/0 | 27445 | 0 | dead-voice 0, 3; no-retrigger v1 (start=5878 len=3328); no-retrigger v2 (start=5878 len=3328) |
| `r-type` | 300 | 4 | 80 | loop/— | 48 | 133/122/114/187 | 32768 | 13383 | no-retrigger v0 (start=60304 len=6672); frozen-voice v2 2.3 s; no-retrigger v3 (start=0 len=1); clipping 0.51% |
| `x-out (title)` | 188 | 7, 65280 | 38 | —/stop | 29 | 36/36/30/25 | 32768 | 140 | frozen-voice v0 9.6 s; frozen-voice v1 10.7 s; frozen-voice v3 9.1 s; stopped-early 6.1 s |
| `turrican 2 title (st)` | 375 | 3 | 62 | loop/— | 90 | 215/79/177/154 | 29122 | 0 | (none) |
| `turrican 2 level 1-desert` | 300 | 4 | 68 | loop/— | 51 | 171/159/10/0 | 29357 | 0 | dead-voice 3; no-retrigger v2 (start=31026 len=1024) |
| `turrican 2 level 3-flight` | 500 | 2 | 17 | loop/— | 32 | 345/313/219/31 | 28672 | 0 | no-retrigger v0 (start=23346 len=1543); no-retrigger v1 (start=0 len=0); no-retrigger v2 (start=66528 len=1504); no-retrigger v3 (start=0 len=0) |
| `turrican 3 level 1` | 1500 | 0 | 45 | loop/— | 44 | 581/615/411/308 | 25082 | 0 | no-retrigger v2 (start=37124 len=2048) |
| `apidya (title)` | 1500 | 0 | 20 | loop/— | 44 | 1050/825/600/375 | 17221 | 0 | no-retrigger v3 (start=0 len=0) |
| `apidya (level 1)` | 300 | 4 | 12 | loop/— | 11 | 200/250/75/50 | 32768 | 900 | no-retrigger v1 (start=35228 len=3072); no-retrigger v3 (start=41372 len=4992) |

`unsupported-ops` and `silence` fired **nowhere**: no module hits a recognized-but-unimplemented
opcode in its first 30 s, and every module produces audible output. `single-pattern` fired nowhere
either — every module walks a real trackstep list.

### What the table says

- **`turrican intro`, the confirmed bug, does not look broken at the sequencer level.** 55 distinct
  trackstep lines, 67 pattern visits, all four voices triggering (206/245/129/205 note-ons), one
  tempo, no stop. Whatever makes it play a different melody than `uade123` is *not* "the trackstep
  layer never got going"; the dispatch is busy and varied. Its one structural finding is
  `no-retrigger` on **voice 1**: 245 note-ons across six distinct macros (`{9, 10, 25, 32, 39, 41}`),
  yet only ever one sample region with DMA on for the whole 30 s. A direct trace check confirms it:
  voice 1's registers do get loaded with a *second* region (`start=17924 len=2048`), but DMA is never
  on for that region — only 20 of 375 jiffy-end snapshots have voice 1's DMA on at all. Six macros
  triggering 245 times and producing one audible region is the sharpest lead the corpus offers for
  this bug, and it is exactly the kind of divergence the A/B by ear described (a missing part /
  wrong patch). **Next thing to chase.**
- **`x-out (title)` stops after 6.1 s** and leaves three voices frozen for 9-11 s afterwards. Its
  tempo set is `{7, 65280}` — `65280` is `$FF00`, which does not read like a tempo at all. A stop
  plus a nonsense tempo value in the same module points at a trackstep-decode problem, not a macro
  one. Second lead, and a self-contained one.
- **`no-retrigger` is noisy as specified** (8 of 10 modules) and should be read with its region
  detail, not as a bare flag. `start=0 len=0` (`apidya (title)` v3, `turrican 2 level 3-flight`
  v1/v3) means DMA is on over an empty region — a voice audibly doing nothing, effectively a dead
  voice the `dead-voice` rule misses because DMA *is* on. A real `start`/`len` (e.g.
  `turrican 3 level 1` v2, one 2048-word region for 30 s) can be perfectly legitimate: a percussion
  or bass voice that plays one instrument throughout. The rule was implemented exactly as the
  roadmap words it ("the sample region never changes for the whole run"); tightening it to "and the
  voice has more than one distinct macro" would cut most of the noise, if a later step wants that.
- **`apidya (title)`'s flagged voice is not the one the ears flagged.** Step 11.2's stems listening
  heard v0/v1/v2 each looping one fragment and v3 silent; `lint` flags only v3 (`start=0 len=0`,
  matching "silent"). v0-v2 each cycle through 3-4 regions in 10 s, so "never changes" does not fire
  for them — the symptom shows up instead as *degenerate* regions, most visibly v2 sitting on
  `start=32005 len=2` (a two-word region on loop) for 97 of its snapshots. This is consistent with
  the TFMX 7V explanation above (garbage region data from a misread format) and is **not** evidence
  of a Pro-2.0 bug. A "degenerate region" finding (`len` of a handful of words) would catch it, but
  it is outside step 11.5's specified list.
- **Clipping is real but mild** on three modules (0.28-0.51% of samples at full scale). Nothing here
  is wall-to-wall clipping; it is peak-limited mixing on loud modules, worth remembering when
  comparing amplitude against a reference render but not a defect on its own.
- **`turrican 2 title (st)` is clean** — no findings at all, 90 pattern visits, all four voices busy.
  It is the corpus's best "known-good" baseline for any A/B work that follows.

### How `lint` is built (so a later step can extend it)

`lint(&[TraceEvent], &[(u8, u32)], &[i16], rate) -> Report` in `tfmx-cli/src/main.rs` is a pure
function: the trace events, the `(opcode, count)` pairs read off `Player::unsupported_ops()`, and
the rendered PCM go in, a `Report` (summary + findings) comes out. The unsupported counters and the
PCM cannot come out of the event stream — they live on the player and in the output buffer — so
they are passed in alongside it rather than being added to `TraceEvent`, keeping the trace seam
what `docs/architecture.md` §2 says it is: state-machine transitions only. Every finding is unit-
tested from a hand-built event vector with no `Player` and no corpus file. A new finding is a new
block at the end of `lint` plus one test; a new output format is a new `write_report`.

`tfmx-cli info` is static as of this step: header text, layout and the song/tempo table, no
playback. Everything that needed a render moved here.
