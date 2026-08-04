# Macro/pattern fidelity: $0B <Portamento> direction bug (r-type, macro 4)

**Status: OPEN, root cause NOT found.** This crate matches the documented formula exactly, but direct editor tests contradict the documented model. See "For whoever picks this up next" below.

[← index](macro-playback-fidelity.md)

---

## 16. NEW, unresolved: `$0B <Portamento>`'s documented model contradicted by direct editor tests — root cause not found

**Started 2026-08-04, unrelated to §15's paused clock-domain thread.** User reported: in `r-type`,
pattern `8` / macro `4` (`voice 3`, the very first note of the whole song — `tfmx-cli trace --voice
3 --gate any` shows it fires at `frame=0`), this crate's portamento glides pitch **down**, but it
should glide **up**.

### The macro in question

```
tfmx-cli disasm --macro 4 "testdata/mdat.r-type" "testdata/smpl.r-type"
   0: $00 <DMAoff+Reset*>
   1: $02 <SetBegin>
   2: $03 <SetLen> bb=$01 cc=$00
   3: $08 <AddNote*>
   4: $01 <DMAon>
   5: $0D <AddVolume> cc=$14
   6: $0B <Portamento> aa=$01 bb=$00 cc=$04      -- every 1 jiffy, rate = +4
   7: $08 <AddNote*> aa=$10                       -- +16 semitones
   8: $11 <AddBegin> cc=$70
   9: $04 <Wait*>
  10: $05 <Loop> aa=$80 target=8
  11-14: $04 <Wait*> x4
  15: $07 <STOP*>
```

`tfmx-cli trace --voice 3 --seconds 5 --gate any` confirms this crate's period climbs steadily and
exactly by the documented formula once DMA turns on: `267 → 271 → 275 → 279 → … → 323` (each step
= previous × 260/256, truncated — an exact match). Period growing = pitch falling, i.e. this
crate's render descends, matching the user's report. **This is not a bug in the multiply loop —
the code does exactly what `docs/playback-model.md` §5.3 documents** (`period *= (256+bb)/256`,
positive `bb` bends pitch down, sourced from `[S1]`).

### Ruled out: automated pitch-tracing against `uade123`

Tried to settle the sign question objectively rather than by ear. `uade123` (installed locally) can
play the real `r-type` module, but:
- A from-scratch synthetic probe module built to isolate the question (single held note,
  `$0B aa=1 bb=+32`, clean sine sample — `testdata/synth/gen_portamento_probe.py`, generates
  `mdat.portamento-probe`/`smpl.portamento-probe`) is **rejected by `uade123`** ("module check
  failed") even though the fs-uae TFMX editor loads it fine — a module-detection-heuristic gap in
  `uade123`, not a bug in the fixture.
- Autocorrelation pitch-tracking (`tfmx-cli measure-pitch`, and a custom numpy autocorrelation
  script restricted to a sensible frequency band) on the real `r-type` render — both full-mix and
  isolated to voice 3's hard-panned channel (`docs/playback-model.md` §2.1: voices 0/3 = left,
  1/2 = right) — produced noisy, non-monotonic readings on `uade123`'s output, unlike this crate's
  own render (which traces a perfectly clean monotonic curve, as expected since it's driven by the
  same formula the tool measures). The real instrument's sample content is evidently not tonal
  enough for autocorrelation to lock onto a stable fundamental. **This approach was abandoned as
  unreliable** — it cannot currently be used to confirm/deny the glide direction on real corpus
  audio.

### Editor experiments (decisive, but incomplete)

The user then tested directly in the fs-uae TFMX editor, using the `portamento-probe` module above
(`SetNote $1E → DMAon → $0B aa=1 bb=$0020 → Wait 100 → STOP`) and hand-edited variants:

1. **`$0B` alone (no note-set instruction after it) produces no audible glide at all** — just a
   static held note, DMA/sample playback otherwise fine. This directly contradicts this crate's
   current model, which ticks portamento every jiffy regardless of macro suspend state
   (`MacroInterpreter::tick`, `tfmx/src/macro_interp.rs:572-574`) as soon as `$0B` has executed —
   our engine *should* audibly glide in this exact case, and doesn't need any subsequent
   instruction to do so. **Real TFMX apparently needs something else — most likely a following
   `AddNote`/`SetNote` — to make `$0B` do anything perceptible.**

2. **Adding `$08 <AddNote>` directly after `$0B` does trigger an audible glide.** Starting note via
   `SetNote $10`, `$0B aa=1 bb=$0F` (rate = +15), then sweeping `AddNote`'s own `aa` operand
   (**not** Portamento's — a different opcode, different byte) from `$00` to `$3F` produced this
   direction table (user-confirmed, corrected for an earlier transcription slip):

   | `AddNote aa` | direction |
   |---|---|
   | `$00`–`$21` | up |
   | `$22`–`$31` | down |
   | `$32` | up |
   | `$33`–`$39` | down |
   | `$3A` | up |
   | `$3B`–`$3F` | down |

   Checked numerically against this crate's own `note_period()`: modeling direction as
   `sign(bb) × sign(current_period − target_period)` (target = `note_period(0x10 + aa)`) matches
   **all 33** cases in the first, best-behaved band (`$00`–`$21`) exactly — in every one of those,
   the target note is a sane, in-range transpose and the target period is below the current period
   (673), predicting "up," which is what was observed. The mismatches start exactly at `$22`
   onward, where `note_period()`'s smooth exponential extrapolation (never validated against real
   hardware much past the documented `$00`-`$3F` note range — see its own doc comment,
   `tfmx/src/macro_interp.rs:19-20`) most plausibly diverges from whatever the real, presumably
   table-based, note→period mapping does out that far — not evidence against the sign model itself.

3. **Confirmatory flip test**: same setup, same `AddNote aa=$08` (a confirmed "up" case), only
   `$0B`'s `bb` changed from `+$0F` (`$000F`) to `-15` (two's complement `$FFF1`) — **direction
   flipped to down.** This rules out "direction is purely target-vs-current, `bb`'s sign is
   irrelevant" (which had fit observation 2's first band on its own): if `bb`'s sign didn't matter,
   this flip should have changed nothing.

4. **User's own follow-up observation, after more experimentation**: `bb`'s sign does **not**
   reliably predict direction either — "some current state influences the decision" — and at this
   point neither of us has a model that explains all the data. In particular, the open question
   asked at the end of the previous session (does the glide converge/settle near the `AddNote`
   target, or run away indefinitely with no sign of leveling off — which would discriminate a
   target-seeking exponential-approach model from a plain open-ended multiply with a
   context-dependent sign) was **never answered** before the investigation was paused here.

### Where this leaves things

**Root cause not found.** What's solid:
- The original bug report stands: this crate's `$0B`/`$FC <Portamento>` glides the wrong direction
  for at least the `r-type` pattern `8`/macro `4` case, and this crate's implementation exactly
  matches what `docs/playback-model.md` §5.3 currently documents — so if this is a bug, it's a
  **documentation/understanding bug inherited from how `[S1]` was read**, not a coding slip.
- `$0B` almost certainly needs a state-machine redesign, not a sign flip: real behavior needs a
  following note-set op to produce any audible effect at all, which this crate's "ticks
  unconditionally once armed" model (`tfmx/src/macro_interp.rs:111-143`, `572-574`) cannot
  reproduce as-is.
- Direction depends on more than `bb`'s literal sign (item 3) and more than target-vs-current alone
  (item 4) — some third factor, or an interaction not yet identified, decides it. Candidate ideas
  floated but **not tested**: an exponential-approach-to-target formula (`remaining *= (256∓bb)/256`
  where `remaining = period − target`) fits data points 2 and 3 but was never checked against the
  convergence question in item 4; a real, non-extrapolated note→period table with its own
  wrap/aliasing behavior past a certain range remains unconfirmed.
- `$FC <Port>` (the pattern-level twin, `tfmx/src/sequencer.rs`/`tfmx/src/player.rs:445-447`)
  shares the same `Portamento` struct and was not tested separately — assume it has the same
  problem, don't assume the same fix transfers untested.
- Do **not** trust `docs/playback-model.md` §5.3's stated sign convention as verified going
  forward — it is now contradicted by direct editor evidence, even though it's a faithful
  transcription of what was read from `[S1]`.

### For whoever picks this up next

1. Get more systematic editor data before hypothesizing further: vary the **starting** note (not
   just `$10`) and vary `bb`'s **magnitude** (not just `±15`), not only `AddNote`'s target — the
   existing data set only ever varies one or two of the three inputs at a time, which is why two
   plausible-looking models have already been individually disproved.
2. Settle the convergence question from item 4 above — does the glide stop/settle near the target
   note, or run away without limit? This alone discriminates several candidate mechanisms.
3. Re-read `[S1]`'s `$0B`/`$FC` sections (and its §4 diagram, if any — `docs/opcodes.md`'s own
   citation at lines 216-229 quotes the operand layout but not a worked multi-note example the way
   §5.3's single worked example only ever showed one static multiply, never a note-change
   interaction) specifically for any mention of a following note op being required, or of any
   state `$0B` reads besides `bb`/`aa` and the current period.
4. `testdata/synth/gen_portamento_probe.py` (→ `mdat.portamento-probe`/`smpl.portamento-probe`) is
   still a valid, editor-loadable fixture for further hand-edited experiments — `uade123` cannot
   load it (see above), the fs-uae editor can.
5. This is unrelated to and does not block §15's paused clock-domain thread — both are open at the
   same time, on different macros/patterns.

