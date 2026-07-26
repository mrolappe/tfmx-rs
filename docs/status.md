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

## Update (2026-07-26, step 11.6): mutation robustness test, and the `0x7F` mask is no longer the panic guard

`tfmx/tests/mutation_robustness.rs` is a new integration test: a hand-rolled 64-bit LCG (no
dependency) flips one random byte in a clone of each corpus `mdat` or `smpl` buffer, 300 times per
module across all 10 modules (3,000 mutations total), and asserts that `Module::parse` followed by
a 1 s `Player::render` never panics — `Err` from either call is accepted, only a panic fails the
test. It passes clean against the current corpus and current code (~13 s in debug).

**The step's own suggested negative control does not reproduce.** Reverting the `number & 0x7F`
mask in `sequencer.rs::decode_track_word` (letting an unstated `$81`-`$FD` trackstep byte reach
`PatternRunner::new` unmasked) does **not** make the test fail: `Module::pattern` /
`pointer_table_entry` already reject any out-of-range pattern number with `Err(AccessError::OutOfRange)`
via checked arithmetic and `slice::get`, so the value flows through as a normal error return, not a
panic. Tracing it further, every pointer-table and trackstep-line accessor in `module.rs`
(`trackstep_line`, `sample`, `pointer_table_entry`) is already `checked_add` + `.get()` all the way
down, and every corpus-derived array index elsewhere (`voice_of`'s `& 0x03`, `UnsupportedOps`'
256-entry table, `Sequencer::new`'s `song >= 32` guard) is independently bounds-checked. The mask is
now a *value-tolerance* decision (whether `$81`-`$FD` is accepted as a masked pattern number or
rejected outright), not a safety-critical guard — that guard already lives in `pointer_table_entry`
and was evidently hardened after the step 6.1/10.1 incidents the roadmap cites. This is a good sign
for the codebase, not a gap in the test: to confirm the harness itself still catches real
regressions, `Module::trackstep_line`'s `.get(start..end)` was temporarily swapped for direct
`&self.mdat[start..end]` indexing (reintroducing exactly the step-10.1 bug class) and the mutation
test failed immediately, reporting the panicking module, mutation index, byte offset and flip byte;
the change was then reverted. No source outside the new test file is different from before this step.

## Update (2026-07-26): `turrican intro`'s no-retrigger bug fixed -- `note_on` replaces unconditional `trigger` -- but the full-mix A/B still sounds very different

Root cause of the confirmed bug (voice 1 playing a different melody than `uade123`, first flagged
step 6.2, narrowed by step 11.5's `lint` to a `no-retrigger` finding on voice 1): traced the exact
macro bytecode with a scratch dump of `turrican intro`'s macro 41 (used by an 8-note run at 1 jiffy
per note). Every macro in this module opens with `$00 aa=0` (mandatory 1-jiffy pause,
`docs/playback-model.md` §2.4) and its note-setting opcode (`$08`/`$09`) ends its own jiffy too, so
`$01 DMAon` needs two full clear jiffies after any trigger to ever run. `dispatch_pattern_entry`
(`tfmx/src/player.rs`) called `MacroInterpreter::trigger()` -- a full reset of `step`, `dma_on`, the
sample pointer and every effect -- for **every** `Note` pattern entry, regardless of whether the
same macro was already running on that voice. A note run that retriggers the same macro every
jiffy therefore reset `step` back to 0 every time, and `$01` was never reached.

Confirmed real (not a benign artifact) by an A/B against `uade123` before touching any code: 20ms-
window RMS across the run's 0.5-1.5s span showed this crate's voice 1 completely silent
(`rms=0.0` throughout) while `uade123`'s reference render had continuous non-zero energy over the
same span, no gaps -- and this crate's own full mix had two ~200-300ms silent gaps in that window
that the reference does not.

Fix: `MacroInterpreter::note_on` (`tfmx/src/macro_interp.rs`) -- if the requested `macro_number`
equals the voice's currently-running one and the program hasn't reached `$07 <STOP>`, updates
note/volume/transpose in place instead of calling `trigger()`; otherwise (a different macro, or the
same macro after it stopped) it calls `trigger()` exactly as before. `dispatch_pattern_entry` now
calls `note_on` instead of `trigger` directly. Three new `tfmx` unit tests cover: same-macro retrigger
while still running preserves `dma_on`; a different macro number still fully resets; the same macro
number after `$07` still fully resets. **Uncertain**: no `[S1]` citation states this same-macro
behavior -- it is grounded empirically by the A/B above, not by the published spec.

Verified after the fix: the same A/B window now shows continuous energy on this crate's voice 1
(silent only 0.5-0.78s, its genuine pre-attack pause, then continuous through 1.26s). Corpus-wide
`tfmx-cli lint` (step 11.5's table) re-run post-fix: `turrican intro` now reports **no findings at
all** (was `no-retrigger v1` + clipping). All ten modules' golden hashes changed and were
regenerated (`TFMX_REGEN_GOLDEN=1`) -- an intentional, verified output change, not a regression;
`cargo test --workspace` is green apart from that expected regen. Committed.

**But a fresh full-mix A/B after the fix still sounds very different from `uade123`** -- the
`note_on` fix is real, tested, and closes the specific silence it targeted, but it is not the
(whole) explanation for the original step 6.2 complaint (different melody, different starting
patch, extra percussion voice, higher pitch). This mirrors the earlier `apidya (title)` history in
this same file: a real, correctly-tested fix (the `Voice::frac` reset) that turned out not to be
sufficient on its own. **Do not re-claim this is fixed without a fresh listen confirming it.**

### Song-number mismatch ruled out

Before chasing note/pitch dispatch further, checked whether this crate's `--song 0` and
`uade123`'s default subsong are even the same piece of music -- they could differ if either tool
enumerates/renumbers the header's 32 song-table slots differently.

- `uade123 -g "mdat.turrican intro"` reports `subsongs: cur 0 min 0 max 4` -- 5 subsongs, current/
  default is 0.
- `tfmx-cli info` dumps this crate's own song table: slot 0 = `start=75 end=129 tempo=3`, slot 1 =
  `start=52 end=74 tempo=120`, slot 2 = `start=0 end=49 tempo=160`, slot 3 = `start=50 end=50
  tempo=5`, slot 4 = `start=138 end=138 tempo=3`, slots 5-30 all identical to slot 3, slot 31 =
  `start=511 end=511 tempo=5`. `uade123`'s "5 subsongs" is exactly the count of slots before that
  placeholder run starts repeating -- strong evidence its subsong index is this crate's raw slot
  index, not a renumbering.
- Independent cross-check by note density (a wrong song would be tempo-wildly-off, not subtly
  wrong): this crate's song-0/1/2 trigger rates from `tfmx-cli trace` are 26.1/s, 81.2/s, 132.2/s
  respectively (10s render each). Onset rate measured directly from `uade123`'s reference audio
  (20ms-window RMS envelope, simple threshold-jump onset counter) is **27.1/s** -- matches song 0
  almost exactly; songs 1 and 2 would be obviously, grossly faster.

**Conclusion: song numbering does correspond (song 0 == uade123 subsong 0).** The remaining
discrepancy is genuinely in note/pitch/patch dispatch, not a song-selection bug.

### Tempo-table semantics clarified (in the course of the above)

Answering "why does tempo 3 vs. tempo 160 look so different" turned up nothing new beyond
`docs/playback-model.md` §3.2, but confirms it against this specific module's own data: values
`<=15` use the 50Hz-divider path (`50/(v+1)` Hz -- tempo 3 -> 12.5 Hz jiffy rate, not a literal
"3 BPM"), values `>15` use the CIA/BPM path where the stored value **is** literally the BPM
(tempo 160 -> 160 BPM, a normal fast action-game tempo). Slot 0 (tempo 3, lines 75-129) and slot 2
(tempo 160, lines 0-49) are most likely two separate pieces of music sharing one `mdat` file (a
slow intro/title cue and a fast in-game tune), not one tune with an inconsistent tempo.

Also checked whether `start == end` reliably means "dummy/empty slot": **no, it's mixed.** Traced
slot 3 directly: line 50 decodes as `TRACKSTEP Command(Stop)` -- genuinely empty, and 27 of the 32
slots point at this exact value, almost certainly the composer's tool default-filling unused song
slots. But slot 4 (`138/138`) and slot 31 (`511/511`) are also `start==end` and traced to **real**
`Tracks([...])` lines with genuine note/pattern content -- legitimate one-line songs that loop that
single trackstep line forever, not dummies. The reliable dummy signal is the line decoding to
`Command(Stop)`, not the `start==end` shape by itself.

### Next task: keep digging on the note/pitch/patch discrepancy

Not yet tried, in likely order of leverage:

1. **Per-voice stems A/B** (`tfmx-cli render --stems`) against a fresh `uade123` render of the
   same song/duration, voice-by-voice -- the same technique that found the `note_on` bug, applied
   more broadly across the whole render rather than one 1-second window.
2. **Transpose handling** -- `dispatch_pattern_entry`/`PatternRunner` transpose plumbing
   (`tfmx/src/sequencer.rs`, `tfmx/src/player.rs`) has not been A/B-verified against a reference;
   a sign or accumulation error here would produce exactly "higher pitch" and "different melody"
   without touching timing at all.
3. **Note-to-period mapping** (`note_period` in `tfmx/src/macro_interp.rs`) -- not yet checked
   against a known-good frequency table for more than a couple of spot values.
4. **Which pattern/macro is actually selected for the "starting patch"** the user described --
   compare the very first few `TRIGGER`/`PATTERN` events this crate emits (`tfmx-cli trace`)
   against what `uade123`'s stems (if extractable) or its audible attack timbre suggest for voice 0
   at t=0.
5. The still-untouched `Portamento`-timed note case noticed in passing while investigating the
   `note_on` fix: `NoteTiming::Portamento` (`aa >= 0xC0`, "reached by portamento from the previous
   note ... rather than played directly", `docs/opcodes.md` §2) is decoded but never specially
   handled -- `dispatch_pattern_entry` calls `note_on` the same way regardless of `timing`. This is
   a second, independently plausible dispatch bug, not yet confirmed against the corpus.

## Update (2026-07-26, later): four candidates ruled out, master-volume slide found as the likely real cause -- NOT YET FIXED

Continuing the `turrican intro` full-mix discrepancy with a fresh A/B: rendered `--stems` (per
voice, 15s) plus a 15s trim of a fresh `uade123` reference render. **User's listen: still very
different -- `crate-v3.wav` starts more or less immediately with bass-drum-like kicks, while the
`uade123` reference only has audible percussion near the end.** This is a stronger, more
structural symptom than the earlier pitch/melody complaints, and narrowed the search.

**Ruled out by code inspection against the spec** (none of these are the cause for this module):

- **Transpose plumbing** (`$08`/`$09`/`$1F` in `tfmx/src/macro_interp.rs`, trackstep/`$FB PPat`
  transpose in `tfmx/src/sequencer.rs` and `tfmx/src/player.rs::dispatch_pattern_entry`): every
  formula matches `docs/opcodes.md`'s wording exactly (`$08` = `self.note + aa + track_transpose`,
  `$09` = `aa + track_transpose`, `$1F` = `self.last_note + aa + track_transpose`).
- **`note_period`** (`tfmx/src/macro_interp.rs`): formula and the detune/finetune multiplier
  (`1 + signed_value/256`) match `docs/playback-model.md` §4's two worked examples exactly.
- **`$FB <PlayPattern>` (PPat)**: still a real gap (dispatched as a no-op in
  `tfmx/src/player.rs`, same bucket as `Fade`/`Lock`) -- but a 120s trace of `turrican intro` song
  0 (`tfmx-cli trace --seconds 120`) contains zero `PlayPattern`/`Fade`/`Lock` commands. Real
  missing feature, not this module's bug.
- **`NoteTiming::Portamento`-timed notes**: same 120s trace shows only `Detune`/`Wait` timing,
  never `Portamento` -- this module never encodes that case.
- **`$1C <Splitkey>` dispatch on voice 3's drum-like hit**: traced macro 24's raw bytecode
  (dumped via a scratch `Module::macro_` reader, same technique as the earlier macro-41 dump) --
  it's a keysplit: `Splitkey(aa=$20, target=step 2)`, i.e. "if note < 32, jump to step 2 (Cont
  macro 23), else fall through to step 1 (Cont macro 22)". Every voice-3 trigger in the trace
  carries `note=32` (not `<32`), so it correctly falls through to macro 22 -- `self.note` is set
  before the macro program runs (`MacroInterpreter::trigger`/`note_on`), so this is not a stale-note
  bug. Macro 22's own bytecode is a rapid one-`$08 AddNote`-per-jiffy transpose sequence
  (0, +5, 0, -2, -3, -4, -5) -- a classic descending pitch-sweep, a very plausible deliberate
  percussion/kick-style instrument, not an accidental wrong-instrument bug.
- **Pitch cross-check via autocorrelation (abandoned as unreliable, do not reuse as-is)**: tried
  extracting monophonic per-voice runs from `tfmx-cli trace`'s `VOICE` period field and comparing
  the expected frequency (`3546895/period`) against autocorrelation on (a) the `uade123` reference
  and (b) this crate's own `--stems` render. (a) found zero usable monophonic windows in the
  reference (polyphonic throughout). (b) found every stem's estimated pitch a clean power-of-two
  fraction of expected (~1/16, ~1/32) -- traced to short `loop_len` wavetable-style instrument
  samples (e.g. `loop_len: 8` words = 16 bytes) fooling naive autocorrelation into locking onto a
  short-lag harmonic instead of the true fundamental. A flaw in the analysis script, not evidence
  of an engine bug.

**Likely real cause, found but NOT YET IMPLEMENTED**: `turrican intro`'s trackstep line 75 (the
song's very first line) decodes to `LineCommand::MasterVolSlideB { divisor: 0, target: 64 }`
(confirmed via `tfmx-cli trace`). `tfmx/src/sequencer.rs:347-353` already has a standing comment
admitting this is a known no-op: *"Nothing in this crate yet consumes a master-volume slide (there
is no master-volume concept on `Paula`)"*. Consequence: every note plays at its own per-voice
volume with zero overall attenuation from jiffy 0, including voice 3's drum-like hit at trackstep
line 77 (just 2 lines into the song). A composer opening a song with "slide master volume to 64"
as literally the first command is the classic fade-in idiom -- it only makes sense if the volume
starts below 64 and ramps up, which would make early hits (like that drum) much quieter or
inaudible in a spec-correct player while this crate blasts them at full volume immediately. This
lines up with the user's exact symptom.

**Open uncertainty**: `[S1]` states the master-volume-slide mechanics (`docs/playback-model.md`
§5.1: "every `divisor` jiffies, master volume moves by 1 towards `target`") but never states the
*default* master volume before any slide runs. If it already defaults to 64 (max), this slide
would be a no-op and the whole theory is wrong. Tried to check this against the original TFMX
editor (v1.5, via UAE) as an independent ground-truth source (distinct from `uade123`, which is
just another replayer implementation) -- **the editor would not play `turrican intro` at all**
(likely a version/format mismatch on the editor's own side, not informative either way). No
one-file-vs-two-file mdat/smpl variant was available to test that unrelated tangent either;
`docs/format.md` and this crate are scoped to the two-file layout only, and nothing here suggests
that matters for `turrican intro` (which is itself two-file).

### Next task, next session: implement master volume, default 0

Plan agreed with the user, explicitly deferred to a fresh session (not started this session):

1. Add master-volume state (consumed by `$EFFE 0003`/`0004` line commands and pattern `$FA <Fade>`,
   which share the identical "every N jiffies, move by 1 towards target" mechanic per
   `docs/playback-model.md` §5.1) -- most natural home is `Sequencer` (the line commands are
   sequencer/trackstep-level) or a small new struct threaded into the final mix in `player.rs`;
   `Paula` itself has no master-volume concept today and this doesn't obviously belong on a
   per-voice `Paula::Voice`.
2. Default the master volume to **0** at song start (the only default consistent with the
   fade-in-to-64 read above) unless/until better evidence surfaces.
3. TDD per the project's hard rule: a unit test driving `$EFFE 0003/0004` and `$FA` slide-by-1
   behavior and clamping, plus wiring the resulting value into whatever combines per-voice Paula
   output into the final sample stream.
4. Render a fresh full-mix + stems A/B for `turrican intro` song 0 and get the user's ear
   confirmation -- per `[[feedback_verify_audio_before_claiming_done]]`, do not claim this fixes
   the discrepancy without a fresh listen. If default-0 is wrong, it should be obvious immediately
   (either no change, or overcorrects to near-silent).
5. If master volume turns out insufficient on its own (following the pattern of every previous
   "real but not sufficient" fix in this investigation), the next untried item is comparing the
   very first `TRIGGER`/`PATTERN` trace events against the reference's audible attack timbre at
   t=0 (never attempted this session).

## Update (2026-07-26, next session): master volume implemented, default-0 theory falsified

Implemented the plan above (TDD, per the project's hard rule): `Paula` now owns `master_volume`
(0..=64) and an optional slide, reusing `macro_interp::Envelope`'s existing "every `every` jiffies,
move by 1 towards `target`, clamp on arrival" mechanic (made `pub(crate)`) rather than
re-implementing the same shape a third time. `Sequencer::advance` still only recognizes and times
`$EFFE 0003`/`0004`, as before; `Player::run_jiffy` now reads the returned `LineCommand` and starts
the slide on `Paula`, and `dispatch_pattern_entry` does the same for pattern `$FA <Fade>` (both
share the identical mechanic per `docs/playback-model.md` §5.1). New tests: `Paula`-level slide/
clamp/render-scaling unit tests, plus a player-level test proving the trackstep wiring end to end
against a synthetic module (not against `turrican intro`'s own data -- see below for why).

**Step 2 of the plan (default to 0 at song start) was tried and found wrong before any listening
was needed.** `Player::new` defaulted `Paula`'s master volume to 0; `cargo test --workspace`'s
golden-hash regression immediately caught it: `apidya (level 1)` (confirmed **TFMX Pro**, not the
unrelated 7V `apidya (title)` -- `docs/status.md`'s step-11.2 update, `apidya (level 1) | TFMX Pro`)
never issues a master-volume command anywhere in a 15 s trace, and under the default-0 policy
rendered **completely silent** (`tfmx-cli lint`: `peak amplitude 0`) despite ~280 real note-ons
across all four voices. A crate-wide default below 64 is inconsistent with any module that manages
master volume only implicitly -- which is most of the corpus. Corrected: `Player::new` now stands
on `Paula::new`'s own neutral default (64, no attenuation); only a module that explicitly starts a
slide ever moves away from it.

**Consequence for the `turrican intro` investigation: the fade-in theory is falsified, not
confirmed.** With the corrected (64) default, `turrican intro`'s trackstep line 75
(`MasterVolSlideB { divisor: 0, target: 64 }`) is a genuine no-op -- current already equals target,
so the slide starts and immediately drops itself. This master-volume mechanism is now real and
correctly wired for any module that does use it non-trivially, but it does **not** explain
`turrican intro`'s drum-at-the-start symptom. Re-verified via the full workspace test suite +
golden-hash diff: of all 10 corpus files, only `apidya (title)`'s hash changed (that file's parser
already reads 7V data as Pro-2.0 garbage -- `docs/status.md`'s step-11.2 update -- so a garbage
`$EFFE`/`$FA` sequence newly doing *something* there is expected noise, not a regression); the
other 9, including `turrican intro` and `apidya (level 1)`, are byte-identical to before this
session's change.

**Next untried item, still open, next session**: step 5's own fallback -- compare the very first
`TRIGGER`/`PATTERN` trace events against the reference's audible attack timbre at t=0. Master
volume as a mechanism is done; it is no longer the lead suspect for this symptom.

## Update (2026-07-26, next session): first-note timing gap found, real bug fixed, still insufficient

Did the t=0 attack-timbre comparison the previous update deferred. `uade123`'s `turrican intro`
render is audible at t≈80ms (1 jiffy after song start); this crate's render stays silent until
t≈240ms (2 jiffies later) -- confirmed both by ear (`afplay` A/B) and by RMS-envelope onset
detection on both renders. Traced this precisely: voice 2's first note uses macro 48, whose
bytecode is `$00 aa=0` (mandatory 1-jiffy pause) → `$02/$03/$0D` setup → `$08 AddNote` (asterisked
"ends macro processing for this jiffy") → `$01 DMAon`. Given trigger at jiffy 1 (trackstep line
76), this crate's macro interpreter cannot reach `$01` before jiffy 3 (240ms) under a literal
reading of `docs/opcodes.md`'s `$00`/`$08` semantics -- confirmed by dumping the raw bytecode via a
throwaway example (not committed) and hand-tracing `MacroInterpreter::tick`/`take_turn` jiffy by
jiffy. **Root cause of the 160ms gap itself is still open** -- our decode matches the spec text, so
either that reading is subtly wrong, `uade123`'s 80ms onset is a different note than we assume, or
the trackstep line-advance timing (`docs/playback-model.md` §7's open question on what actually
triggers a line advance) is off by a jiffy at song start. Not chased further this session because
a 10-second full-mix spectrogram comparison (below) turned up a bigger, more concrete lead.

**User's ear confirms the bigger problem is not the timing gap.** Asked directly after an A/B
listen: the instrument timbre/melody being wrong is "the latter" -- i.e. the dominant issue, not
the 160ms late start. This matches the phase-gate text's still-open "different melody" complaint.

**Spectrogram comparison (10s, `NFFT=2048`, generated via a throwaway numpy/matplotlib venv, not
committed) found a striking structural difference**: `uade123`'s render is continuously dense --
some voice is always sounding across the full 10s. This crate's render has multiple clear total-
silence gaps (visible as blank vertical bands) around t≈1.3s, 2.2-2.6s, 3.6-3.7s, 6.3-6.7s, none of
which align with the song's own trackstep loop-back point (line 129→77, confirmed via `tfmx-cli
trace`'s `JIFFY` events at frame 194040 ≈ 4.4s and 381024 ≈ 8.64s) -- these gaps happen *within* a
single pass through the repeating section, not at the loop seam.

**Found and fixed one genuine bug while chasing the gaps, but it does NOT explain them.** Dumped
macro 28's bytecode (voice 0's instrument, active right around the 2.2-2.6s gap): `$03 SetLen 1024`
followed later by `$18 Sampleloop` with a 24-bit delta of `+1792`. `$18` ("Adds `aaaaaa` to the
sample start address, and subtracts the same value from the sample length", `docs/opcodes.md` §3)
subtracts 1792 from a `loop_len` of only 1024 -- a genuine underflow the composer's own data
triggers unconditionally (this `$18` runs exactly once per note, not inside a loop, so it's not a
"compounds until it eventually goes negative" case; it's designed to go negative on this one call).
`MacroInterpreter`'s `0x18` handler did `self.loop_len.wrapping_sub_signed(delta)` on a bare `u32`,
which wraps mod 2^32 (1024 - 1792 → 4294966528), not mod 2^16 like Paula's actual 16-bit length
register (`docs/format.md` §8). A length that large makes `Voice::next_sample`
(`tfmx/src/paula.rs:70-77`) never reach the wrap-back-to-loop-start condition; it just reads
forward past the real sample buffer forever, and `smpl.get(...).unwrap_or(0)` (paula.rs:62-63)
silently returns 0 from that point on -- permanent silence for that voice until its next full
`trigger()`. Confirmed by cross-referencing the exact wrapped value (4294966528 = 2^32 - 768,
matching 1024 - 1792 = -768) against the live `tfmx-cli trace` output for that voice at that
timestamp -- not a guess. Fixed (TDD: failing test first) by masking both `0x12 <AddLen>`'s and
`0x18 <Sampleloop>`'s results to `& 0xFFFF`, matching the real 16-bit register width; new test
`sampleloop_underflow_wraps_at_16_bits_like_real_paula_hardware` in `tfmx/src/macro_interp.rs`.
Full workspace test suite (including the golden-hash regression) passes unchanged.

**But: byte-for-byte identical rendered output before and after the fix, for `turrican intro`'s
first 10 seconds** (`cmp` on the two WAVs). The corrupted `loop_len` value is set correctly per
the (buggy) old code, but this specific note's attack region (the `sample_len`/`sample_start` set
by the *second* `$02`/`$03` pair, not the pre-`$18` one) never finishes playing out before the
voice gets retriggered by the next note -- so `next_sample`'s attack-vs-loop switch (paula.rs:72)
never actually reads the broken `loop_len` within this 10-second window. The fix is real, correct,
and worth keeping (it will matter for any note whose attack region *does* play out fully before a
retrigger, and is simply more faithful to real hardware), but it is **not** the explanation for the
observed silence gaps. Matches this investigation's established pattern: every fix found so far has
been real but insufficient on its own.

**Next steps, still open, next session**: the silence gaps' actual cause is still unknown. Good
next moves, not yet tried: (a) pick one gap (e.g. 2.2-2.6s, the longest) and trace every voice's
`dma_on`/`StopChannel`/`StopVoice` state jiffy-by-jiffy through it, since patterns dispatch
`StopChannel` liberally in this corpus and a bug causing *all four* voices to receive one
simultaneously (when the composition likely only intended one or two) would produce exactly this
symptom; (b) revisit whether `MAX_MACRO_OPS_PER_JIFFY` or the per-jiffy dispatch order ever silently
truncates a track's events. The first-note 160ms timing gap (this update's opening finding) remains
unexplained too and may or may not be related -- not yet tested whether it recurs at every
subsequent note attack or was a one-off song-start artifact.

## Update (2026-07-26, next session): a second retrigger bug found and fixed via next-step (a)

Did next-step (a) above: wrote a throwaway script (`tfmx-cli trace`'s text output, parsed to track
each voice's `dma_on` across jiffies -- not committed) and confirmed the hypothesis concretely
against `turrican intro`'s first 3s: at t=0.72s, t=1.28s, and t=2.00-2.08s, **all four voices have
`dma_on == false` simultaneously** -- these are the spectrogram's silence gaps, not an artifact of
the earlier eyeballed timestamps.

Traced the t=0.72s gap to its cause and found a second real bug in the same area as the `note_on`
fix (`5da3623`). Voice 3's percussion instrument is macro 24, whose bytecode (dumped via a throwaway
`tfmx/examples/dump_macro.rs`, not committed, same technique as the earlier macro-41 dump) is:

```
step 0: 1C 20 00 02   <Splitkey> aa=0x20: if note < 32, jump to step 2
step 1: 06 16 00 00   <Cont> macro 0x16 (22), step 0   -- the fallthrough (note >= 32)
step 2: 06 17 00 00   <Cont> macro 0x17 (23), step 0   -- the split target (note < 32)
step 3: 07            STOP
```

Both `$1C` and `$06` run in the *same* jiffy as the trigger (neither suspends -- `docs/opcodes.md`
only marks `*`-flagged opcodes as ending processing for the jiffy, and neither is flagged). So one
jiffy after `trigger(24, ...)`, `self.macro_number` has already moved on to 22 (or 23) -- `$06
<Cont>`'s handler (`tfmx/src/macro_interp.rs`, opcode `0x06`) does `self.macro_number = b1`. The
`note_on` fix (`5da3623`) compares the pattern's incoming macro number against `self.macro_number`
to decide "is this instrument still running" -- for macro 24 that comparison is `24 ==
self.macro_number`, which is `24 == 22`, always false, so **every** retrigger of this instrument
takes the full-`trigger()`-reset branch, even while it is legitimately still running. Macro 22
itself opens with the same `$00 aa=0` mandatory 1-jiffy pause as every other macro in this corpus, so
a fast retrigger (this pattern retriggers roughly every 3-4 jiffies while the composition demands
a rapid percussion run) never survives long enough to reach its own `$01 DMAon` -- the exact same
symptom class as `5da3623`, but reached through `$06 <Cont>` indirection rather than a literal
same-macro-number repeat.

**Fixed** (TDD: failing test first, `note_on_retriggering_through_a_cont_indirection_does_not_reset_dma`
in `tfmx/src/macro_interp.rs`): added a `MacroInterpreter::instrument` field that records the macro
number a pattern's Note event last *triggered* this voice with, separately from `macro_number` (the
live program-counter's macro slot, which `$06 <Cont>` legitimately mutates). `trigger()` sets both;
`$06 <Cont>` only ever touches `macro_number`. `note_on` now compares the incoming macro number
against `self.instrument`, not `self.macro_number`. Full workspace suite green; golden-hash diff
changed for exactly `turrican intro`, `turrican 2 title (st)`, and `turrican 3 level 1` -- the three
corpus modules sharing this keysplit/Cont percussion idiom -- with every other module (including
`apidya (level 1)`, `turrican outside`, `turrican 2 level 1-desert`/`level 3-flight`) byte-identical,
a plausible and targeted signature for a real fix rather than noise. Regenerated
(`TFMX_REGEN_GOLDEN=1`), intentional.

**Re-ran the same `dma_on` trace after the fix: the t=0.72s gap is gone** (voice 3 now stays
`dma_on: true` continuously across that stretch's retriggers, confirmed by the trace no longer
showing an all-false row there). **The t=1.28s and t=2.00-2.08s gaps are still present**, unchanged.
Traced t=1.28s directly: at that trackstep line, voice 1 is explicitly silenced by a `StopVoice`
command (legitimate, composer-authored, not a bug), while voices 0, 2, and 3 are each freshly
retriggered by a note whose *own* macro had already reached `$07 <STOP>` on its own (not via the
`$06 Cont` bug -- confirmed no `Cont`-indirected macro is involved at this specific line) -- so all
three take the correct, spec-faithful full-reset path and pay their own few-jiffy `$00`-pause attack
latency, and by chance/composition all four voices' individual dead zones overlap at this particular
trackstep line. **Not yet determined whether this overlap is intentional (a genuine musical "breath")
or itself a bug** (e.g. something making per-voice attack latency longer than it should be, or making
independent voices land on the same trackstep line more often than the composition intends) -- this
is a different mechanism from both retrigger bugs fixed so far and was not chased further this
session, per the standing preference not to stack fixes without the user's ear confirming progress
first.

**Next steps, still open, next session**: (a) get the user's ear confirmation on this fix before
doing anything else -- per `[[feedback_verify_audio_before_claiming_done]]`, this is real and tested
but, like every previous fix in this investigation, may not be sufficient on its own; (b) if
insufficient, trace the t=1.28s / t=2.00-2.08s gaps' per-voice attack latency more precisely: is each
voice's own `$00`-pause-to-`$01` delay exactly as long as the spec says it should be, or is there a
separate bug inflating it beyond what `5da3623`/this session's fix already account for; (c) the
first-note 160ms timing gap (found two updates ago) is still completely unexplained and untested for
recurrence -- still open, still not chased.

## Update (2026-07-26, same session, continued): added `tfmx-cli disasm`; reference confirms the two remaining gaps are real bugs

**Added a permanent `tfmx-cli disasm --macro N` / `--pattern N` subcommand** (separate commit,
`5c0b35f`), replacing the throwaway dump-and-delete example script this investigation had
hand-rolled twice (macro 41, then macro 24). Patterns reuse `sequencer::decode_pattern_entry`
directly (bumped to `pub`, now exported from `lib.rs`) -- zero new decode logic, same
`PatternEntry`/`PatternCommand` `Debug` output `tfmx-cli trace` already prints. Macros have no
equivalent decoded enum (`MacroInterpreter::execute` goes straight from bytes to inline state
mutation, no intermediate representation to reuse); rather than duplicate that ~30-arm match a
second time, `macro_opcode_name` in `tfmx-cli/src/main.rs` is name-only (mnemonic + raw `aa bb cc`
hex, no semantic decoding of operands) -- a deliberate, noted simplification. Both stop at their own
terminator (`$07 STOP` for macros, `$F0 End`/`$F4 STOP` for patterns) or after 256 steps.

Used it immediately to check whether the two gaps *not* fixed by the `instrument` fix share that
same `$06 <Cont>` root cause: **they don't**. `disasm --macro 28` (voice 0, involved in both
remaining gaps) and `--macro 48` (voice 2, involved in the t=2.0s gap) both go straight `$00` pause
-> setup -> `$01 DMAon`, no `Cont`/`Splitkey` anywhere. Macro 48 even has a deliberate
`$13 <DMAoff>`/`$01 <DMAon>` pulse built into its own internal loop (a tremolo-style hold), unrelated
to pattern-level retriggering. Confirmed: the t=1.28s/t=2.00-2.08s gaps are a *different* mechanism
than what this session already fixed.

**User listened and reported: `uade123`'s reference is *not* quiet at t=1.28s or t=2.00-2.08s.** This
rules out the "genuine musical breath, nothing to fix" possibility raised above -- our silence there
is a real, confirmed-by-ear bug, not a coincidental overlap the composition intends.

**Working hypothesis, not yet tested**: this may be the *same* root cause as the still-unexplained
160ms first-note timing gap (`docs/status.md`'s "Update (2026-07-26, next session): first-note timing
gap found..." section, several updates back), just never previously checked beyond the very first
note of the song. Re-derived the jiffy math precisely: `MacroInterpreter::take_turn`
(`tfmx/src/macro_interp.rs:461`) resolves `Wait::Jiffies(0)` (what `$00 aa=0` sets, line ~576) by
immediately clearing to `Wait::Ready` and returning `true` -- so the jiffy *after* `$00` runs is free
to execute again. For a macro shaped like `$00` -> setup opcodes -> `$08`/`$09 AddNote` (itself
another one-jiffy suspend) -> `$01 DMAon`, this puts DMA-on **two full jiffies after the trigger
jiffy** (trigger jiffy: only `$00` runs; next jiffy: setup + `AddNote`, suspends again; jiffy after
that: `$01` finally runs) -- exactly matching the previously-measured 160ms (2-jiffy) first-note gap.
If this 2-jiffy dead zone applies to *every* fresh `trigger()` throughout the piece (not just the
opening note), it would explain far more than three isolated silence gaps: any time several voices
restart within a jiffy or two of each other, their individual 160ms dead zones stack into exactly the
kind of multi-voice "everything went quiet" symptom this whole investigation has been chasing. This
would mean the three "gaps" found so far are symptoms of one systemic timing offset, not three
separate bugs.

**Explicitly not yet verified** -- this is a hypothesis from re-reading the code and the trace math,
not a confirmed finding. Two live alternative explanations were already on record and not ruled out:
either this crate's reading of `$00`/`$08`'s suspend semantics is subtly wrong (both opcodes are
individually spec-documented as suspending, per `docs/opcodes.md` §3, so this would mean the *published
spec itself* reads differently than assumed, or that two documented one-jiffy suspends don't actually
compound this way on real hardware), or `uade123`'s measured onset in the original first-note test was
a different note than the one assumed (unverified). Do not assume this hypothesis is correct without
testing it.

**Next thing to do, next session (recorded per user request, not yet started)**: pick several
*isolated* single-note attacks later in `turrican intro` (away from the crowded passages already
examined, so the DMA-on jiffy is unambiguous) and compare, for each: (1) this crate's trigger jiffy
and its actual `$01 DMAon` jiffy (via `tfmx-cli trace`, same method as the original first-note
check), against (2) `uade123`'s audible onset for that same note (via `-j <seconds>` to seek the
reference close to it, then listening or an RMS-onset check, same method as the original 160ms
finding). If the ~2-jiffy lag recurs consistently, that confirms the systemic-latency hypothesis and
makes `$00`/`$08`'s suspend semantics (or the trigger-jiffy counting itself) the next thing to
actually fix. If it's inconsistent (sometimes 0 jiffies, sometimes 2), the spec reading is probably
fine for most cases and something else -- possibly per-voice, possibly data-dependent -- is going on;
don't assume the hypothesis without this check. Use the new `tfmx-cli disasm` command (this update)
to inspect whichever macros are involved once specific notes are picked.

**Also still open, unchanged from before**: whether the `$FA`/master-volume/note-timing findings
from earlier sessions interact with this; the `$FB <PlayPattern>` gap (confirmed real but not
triggered by this module, per the "Session 2026-07-26 (later)" section above); and the original
"different melody" phase-gate complaint, which predates all of these timing findings and may or may
not be explained by them once the timing question is settled.

## Update (2026-07-26, next session): the 2-jiffy attack lag is confirmed systemic -- with zero exceptions

Did the recorded next step, but by a stronger method than planned: rather than hand-picking a few
isolated later notes for an audio A/B, wrote a throwaway trace-parsing script (Python, reads
`tfmx-cli trace`'s text output, not committed) that measures the jiffy lag from every *genuine*
`trigger()` call to that voice's own `$01 DMAon`, across the whole first 30s / 785 triggers of
`turrican intro`. "Genuine" means the incoming macro number differs from whatever was last
triggered on that voice -- this guarantees the full `trigger()` reset path (`tfmx/src/macro_interp.rs`
line ~339), not the same-instrument in-place update `note_on` added by `c919266`, which does not
reset `step`/`wait` and would conflate a still-running instrument's own internal DMA pulses (e.g.
macro 48's tremolo, `docs/status.md` above) with attack latency.

First pass produced a confusing scatter (lag values 2 through 5, and a `lag=4` outlier on the very
first note of the whole song). Traced the outlier by hand: voice 0's first-ever trigger (`macro=10
note=21 volume=0`, t=0.08s) never reaches its *own* `$01` at all -- one jiffy later the same voice is
re-triggered by a completely different instrument (`macro=43`), via a full `trigger()` reset, before
macro 10's own DMAon ever fires. The `dma_on=true` my script found a few jiffies later belonged to
macro 43, not macro 10 -- a measurement bug, not an engine anomaly. **This turned out to be the
common case, not an edge case**: of 202 genuine triggers in the sample, 144 (71%) are themselves
superseded by a further re-trigger on the same voice before their own DMAon ever fires. This corpus
multiplexes several logical parts onto few Paula channels by retriggering very fast (this piece's
26/s-ish note rate, already noted a few updates back, divided over 4 voices means a given voice's
"current instrument" often changes every 1-2 jiffies) -- most individual `Note` events in the
trackstep data never actually reach `$01` before something else claims the voice.

Filtering these interruptions out (only counting a trigger's lag if no later trigger touches that
voice before its `dma_on` flips true) gives a clean result: **56 of 56 measurable genuine,
uninterrupted triggers show a lag of exactly 2 jiffies. Zero exceptions.** The two triggers near the
end of the 30s window that show `lag=None` are truncation artifacts (the trace ended before their
own DMAon jiffy arrived), not counter-examples.

**This confirms last update's hypothesis in its clean form**: every fresh `trigger()` in this module
takes exactly 2 jiffies to reach its own `$01 DMAon` -- not "sometimes 0, sometimes 5" as the
unfiltered data first suggested. `$00`'s handler in `tfmx/src/macro_interp.rs` is literally commented
`// <DMAoff+Reset>*` -- turning DMA off is documented as part of what the opcode *does*, not an
accidental side effect of a generic pause -- so this 2-jiffy dead zone at the start of every note is
very likely genuine TFMX/hardware-faithful behavior that a correct `uade123` would also exhibit, not
an implementation bug in this crate's suspend semantics. That reframes the open question: since each
voice's own attack lag looks spec-correct and uniform, the t=1.28s/t=2.00-2.08s all-four-voices-silent
gaps most likely come from *multiple* voices' 2-jiffy windows landing on the same jiffy -- and the
question is whether that overlap is intrinsic to the trackstep data (in which case any faithful
implementation, including `uade123`, should show it too, which would be in tension with "the
reference is not quiet there") or caused by a dispatch-timing difference in this crate that
synchronizes attacks the reference staggers.

**Not yet done, next session**: (a) at t=1.28s and t=2.00-2.08s specifically, list every voice's
trigger jiffy contributing to the gap and check whether the trackstep/pattern data's own timing
values (each track's independent `Wait`/`Detune`/`Hold` offsets) already put them on the same jiffy
by construction, or whether something in this crate's dispatch order nudges them together; (b) if the
overlap is data-intrinsic, revisit whether `uade123` really has zero simultaneous silence there (the
earlier "not quiet" confirmation was by ear, at normal listening levels -- a few 2-jiffy per-voice
dips landing on the same jiffy might be brief enough to not register as "quiet" to the ear even if
technically present); (c) the 71% same-voice-interruption rate is a striking number on its own
and worth a sanity check independent of the silence-gap investigation: is a "note" that never reaches
`$01` before being superseded audible at all (e.g. via portamento/vibrato applied to the *previous*
still-sounding note), or is `dispatch_pattern_entry`/`note_on` silently dropping notes the composition
intended to be heard, which would be a different, new bug class entirely.
