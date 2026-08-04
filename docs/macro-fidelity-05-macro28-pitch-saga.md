# Macro/pattern fidelity: the pattern 0x52/macro 0x1c (macro 28) pitch/out-of-bounds saga — MIDDLE_C_NOTE root cause

**Status: RESOLVED, ear-confirmed.** Long multi-session investigation (original §5→§14) chasing "pitch off, playhead wanders" through several real, structurally-confirmed-but-audibly-inert fixes before landing on the actual cause: `MIDDLE_C_NOTE` was off by a tritone. See [macro-fidelity-08](macro-fidelity-08-pattern52-note-durations.md) for a separate, still-open bug in this same pattern/macro found after this saga closed.

[← index](macro-playback-fidelity.md)

---

## 5. CONFIRMED BUG, found 2026-08-01, root cause FIXED same day (not yet heard): pattern `0x52`/macro `0x1c` on voice 0 is inaudible

**Unresolved disconnect, read this first**: after §3 and §4's fixes landed, the user re-listened
to `turrican-intro-fix.wav`/stems and reported the audible symptoms are essentially unchanged —
"sounds more or less the same as before" (pitch still off, general quality still wrong). Both
fixes are structurally confirmed correct by trace (§3: macro 10's scrub now cleanly oscillates
±256/jiffy; §4: voice 0 now genuinely settles into and stays in loop regions for dozens of
consecutive jiffies). **That structural correctness has NOT translated into an audible
difference the user can hear.** Do not assume §3/§4 are done, and do not assume they're the
dominant cause of the original complaints either — something else not yet identified is very
likely the bigger contributor to "pitch off" and general wrongness. §1 (portamento silently
dropped) is still unfixed and is a strong candidate for at least part of it. This section (§5) is
a second, independently-found, root-caused candidate for at least part of it too.

**The bug, found while investigating a fresh user report** ("I do not hear the notes played by
pattern `0x52` using macro `0x1c` on channel/voice 0"): pattern 82 (`0x52`) plays on track 3, an
infinite pattern-level loop (`$F1 <Loop> aa=0`) of five notes 1-3 jiffies apart, all voice 0,
macro 28 (`0x1c`). Traced with `tfmx-cli trace --song 0 --seconds 90 --gate any` — the notes
genuinely dispatch (`TRIGGER voice=0 macro=28 ...` fires repeatedly), DMA does turn on, volume
settles at a plausible 30/64, and the voice does reach macro 28's own oscillating tail. So this
is not a dispatch bug like §1, nor a region-stomping bug like §4 -- it is a genuine out-of-bounds
read that silently renders as digital zero.

**Root cause, confirmed by direct arithmetic against the real module data**
(`tfmx-cli disasm --macro 28 "testdata/mdat.turrican intro" "testdata/smpl.turrican intro"`):

```
 1: $02 <SetBegin> aa=$00 bb=$1C cc=$14   -- sample_start = 0x001C14 = 7188
 7: $02 <SetBegin> aa=$00 bb=$78 cc=$04   -- += 0x007804 = 30724 -> sample_start = 37912
 8: $03 <SetLen>   aa=$00 bb=$04 cc=$00   -- sample_len = 0x0400 = 1024 (words);
                                             loop_start/loop_len mirror this: 37912 / 1024
13: $18 <Sampleloop> aa=$00 bb=$07 cc=$00 -- delta = 0x000700 = 1792
                                             loop_start = 37912 + 1792 = 39704
                                             loop_len   = 1024 - 1792, wrapped mod 65536 (16-bit
                                                          Paula length register, tfmx/src/
                                                          macro_interp.rs:744-755) = 64768
```

`loop_len = 64768` words = **129536 bytes**. `testdata/smpl.turrican intro` is **45828 bytes
total**. `loop_start (39704) + 129536` reaches far past the end of the file — `Paula::next_sample`
(`tfmx/src/paula.rs:62-63`, `smpl.get(base + pos).copied().unwrap_or(0)`) silently returns `0`
for every sample once `pos` walks past the file's actual end, which (at this voice's period ~252,
~0.32 samples advanced per output frame at 44100 Hz) happens after roughly 0.4s into the loop and
then persists for the many seconds it would take `frac` to wrap the full 129536-sample length
back around. **The note is inaudible for the overwhelming majority of its playback time.**

The `$18` handler's own comment already anticipated a related danger ("when `delta` exceeds the
current `loop_len`, mask to that width so the subtraction wraps mod 65536 like the real chip, not
mod 2^32 (which would produce a length that reads far past the sample buffer and goes silent for
the rest of the note)") -- **but the mod-65536-wrapped result (64768) is itself still far past
this file's actual 45828-byte length.** The safeguard prevents the worse (2^32-scale) overflow but
does not prevent this one. Open questions for whoever picks this up:

- Is `$18 Sampleloop`'s operand/effect decoded correctly at all here? Re-check
  `docs/opcodes.md`'s `$18` row and its §4 diagram against this exact case -- is a 24-bit signed
  delta of `+1792` against a 1024-word `loop_len` (i.e., `delta > loop_len`) actually a case real
  TFMX data is expected to produce, or does it suggest `loop_len` at the time `$18` executes
  should have been something other than 1024 (i.e., a bug earlier in the chain -- was
  `loop_len` supposed to already reflect a different, larger value before `$18` ran)?
- Should out-of-bounds `smpl` reads be a hard error (`AccessError`) instead of the current silent
  `unwrap_or(0)` fallback? That would surface bugs like this one as loud panics/errors during
  development instead of quiet, easy-to-miss silence -- worth weighing against the fact that
  `unwrap_or(0)` may also be intentionally defensive against legitimately-short samples elsewhere
  in the corpus. Check whether any OTHER corpus module/macro has a similarly out-of-bounds
  `$18`-derived loop region before deciding (no existing lint check catches this --
  `tfmx-cli lint` has no "sample region exceeds smpl bounds" finding at all; that itself might be
  worth adding regardless of the root cause here, as a general diagnostic).

**Not fixed.** No code changed in this section -- this is purely diagnostic, recorded for a fresh
session per the user's request.

### Other findings from this same investigation, possibly relevant

- `tfmx-cli lint` on `turrican intro` also reports `frozen-voice: voice 1: period/volume/region
  unchanged for 10.3s with DMA on` and `clipping: 21253 of 2646000 samples at full scale (0.80%)`
  on the current (§3+§4-fixed) render. Neither has been investigated this session; the clipping
  finding in particular could plausibly contribute to a general "off" quality impression alongside
  §1/§5.
- `tfmx-cli info` for this module: `Voice 0: 82 note-ons, macros {1, 10, 28, 43, 44}` -- voice 0
  alone carries five different macros across the render window, several already implicated in
  open bugs (`1` = the portamento-drop macro from §1, `10` = §3/§4's macro, `28` = this section's
  macro). Worth checking macros `43`/`44` too before assuming voice 0 is otherwise clean.

**Update (2026-08-01): this is not a one-macro bug — a new `sample-region-out-of-bounds` lint
finding (`tfmx-cli lint`, `check_sample_bounds` in `tfmx-cli/src/main.rs`, TDD'd) flags every
voice whose attack (`start`/`len`) or loop (`loop_start`/`loop_len`) region reads past the end of
`smpl` while DMA is on.** Swept all ten corpus modules (`--seconds 90`, default `--song 0`,
`AnyTrack` gate): **every single module hits it, on 1 to 4 voices each** (`turrican intro`: 3,
`turrican outside`: 4, `r-type`: 2, `x-out (title)`: 3, `turrican 2 title (st)`: 1, `turrican 2
level 1-desert`: 3, `turrican 2 level 3-flight`: 1, `turrican 3 level 1`: 4, `apidya (title)`: 4,
`apidya (level 1)`: 1 — 26 voice-instances total). Spot-checked `turrican intro` voice 0 with
per-event diagnostics: the flagged region is exactly the `$18`-computed one already root-caused
above (`loop_start=39704 loop_len=64768`, 686 jiffies) confirming the check reproduces the known
case, but the same voice also hits it via a second, smaller, still-unidentified region
(`start` near 44000-56000, `len=1024` words, stepping down by ~1280 bytes across many notes) that
looks unrelated to the `$18` overflow — likely a different macro/pattern, not yet traced back to a
specific opcode. **This reframes §5 from "one inaudible note" to a corpus-wide pattern** — a strong
candidate for a meaningful share of the still-unexplained "pitch/quality off" character noted in
§7, on top of whatever §1's portamento-drop contributes. Root-causing each distinct region (which
macro/pattern, which opcode computed it) is unstarted; the `$18`-wraparound design question from
the original write-up above (decode bug vs. hard-error-the-fallback) still needs a decision before
fixing any of it.

**FIXED, 2026-08-01, TDD'd: the `$18` decode had a real byte/word unit bug — answers the design
question above.** Fresh listen of the trackstep-gating A/B render (`turrican_intro_fixed.wav` +
stems, `--gate any`, full-song, 90s): user reports voice 0 and voice 2 both have "pitch and speed"
wrong, and voice 0 additionally sounds like "the playhead moves to the wrong places in the sample
file" — voice 3 sounds correct. This matches `tfmx-cli lint`'s findings on the nose: voices 0 and 2
are exactly the two flagged `sample-region-out-of-bounds` (voice 3 is also flagged but apparently
not audibly, or not noticed).

Re-examined `$18`'s own arithmetic against `docs/opcodes.md` §4's stated invariant ("moves the loop
start forward/back by `aaaaaa` *bytes* ... shrinking the remaining sample length by the same
amount, so the sample's end point is unchanged") and `docs/format.md` §8's unit citations:
`loop_start` is a **byte** address (same units as `$02 SetBegin`'s 24-bit delta, confirmed
"documented" confidence), but `loop_len` mirrors Paula's length register, which counts **words**
(`$03 SetLen`: "one count of `aaaa` = two bytes", also "documented"). The code
(`tfmx/src/macro_interp.rs`'s `$18` arm) subtracted the raw byte-valued delta from the word-valued
`loop_len` with no unit conversion — which cannot preserve the doc's own stated "end point
unchanged" invariant except when delta happens to be zero. Verified directly against the exact
root-caused case: original loop end = `37912 + 1024×2 = 39960` bytes; with the delta (1792 bytes)
halved before subtracting from `loop_len` (`1024 - 1792/2 = 128` words = 256 bytes), the new end is
`39704 + 256 = 39960` — **exactly unchanged**, versus the old code's `64768`-word (129536-byte)
result that blew straight past the 45828-byte file.

**Evidence this is the right fix, not a guess**: halving the delta before the `loop_len`
subtraction (an arithmetic shift, `delta >> 1`, matching the 68000-idiom rounding convention this
codebase already uses elsewhere for portamento/period math — `docs/playback-model.md` §5.3) and
re-running the corpus-wide `sample-region-out-of-bounds` sweep drops the count from **26
voice-instances to 9**, and resolves both of `turrican intro`'s voice 0 and voice 2 findings that
matched the fresh ear report exactly (voice 0 still has one remaining, different, not-yet-traced
out-of-bounds region per the paragraph above — this fix does not claim to explain that one).

**Fixed, TDD'd**: `tfmx/src/macro_interp.rs`'s `$18` arm now halves the sign-extended 24-bit delta
(`>> 1`, not `/2`, for the ASR-matching rounding convention) before subtracting it from `loop_len`,
still masked mod 65536 to reproduce Paula's 16-bit length-register wraparound. Two existing tests
that encoded the old (buggy) no-conversion arithmetic were updated to the corrected expected values
using even, word-aligned deltas (matching real corpus data); two new tests were added:
`sampleloop_preserves_the_sample_end_point` pins the invariant directly rather than a magic number,
and `sampleloop_keeps_turrican_intro_macro_28_in_bounds` regression-pins the exact real-corpus case
above. All 142 `tfmx` unit tests, `mutation_robustness`, and `tfmx-cli`'s own suite pass. Golden
hashes changed for the six modules that actually exercise an odd/impactful `$18` delta in the
90s render window (`apidya (title)`, `turrican 2 level 1-desert`, `turrican 2 title (st)`,
`turrican 3 level 1`, `turrican intro`, `turrican outside`) and were regenerated; the other four
(`apidya (level 1)`, `r-type`, `turrican 2 level 3-flight`, `x-out (title)`) render byte-identical.

**Not yet heard** — per the standing rule this isn't done until the user's ears confirm voice 0/2
actually sound different (and hopefully closer to `uade123`) now.

---

---


## 8. §5's fix re-listened: NOT audibly different on voice 0/2 either. Basic-playback doubt raised, partly resolved; `$18` ground-truth recipe for next session

**Re-listen result (2026-08-01)**: after §5's `$18` byte/word unit fix, the user re-listened to
`turrican_intro_fixed.wav`'s voice 0 and voice 2 stems specifically (the two the fix targeted) and
reports **no audible change** — "sound more or less the same as before." Same disconnect pattern as
§7: a structurally-confirmed, TDD'd, corpus-measured fix (26→9 out-of-bounds voice-instances) that
doesn't move the ear's verdict. This raised a more basic doubt: **is note playback correct at all**,
since pitch sounds off on seemingly every voice, not just the ones with a specific bug fixed so far.

**Partly resolved same session**: built a new `tfmx-cli measure-pitch` subcommand (autocorrelation-
based fundamental-frequency detector on a rendered WAV, TDD'd — 4 new tests including a
non-sine repeating-waveform case, since raw 8-bit PCM sample loops aren't clean sine tones) and used
it on a from-scratch, hand-built minimal `.mdat`/`.smpl` test (a single macro doing only
`SetBegin`→`SetLen`(4 words/8 bytes)→`AddNote(aa=0)`→`DMAon`→`STOP`, no loops/effects/transpose, an
8-sample ramp as the "instrument") — deliberately bypassing every other layer (trackstep, pattern,
macro effects, real corpus data) to test only `note_on`→`$08 AddNote`→`note_period()`→
`Paula.set_period`→`next_sample`→rendered-PCM, end to end.

Result: `0x12`/`0x1E`/`0x2A` (C-2/C-3/C-4, one octave apart) measured 525.0/1050.0/2100.0 Hz against
a predicted 522.7/1045.4/2090.8 Hz (`8363×2^((note−30)/12)` ÷ 8 samples/loop) — **octaves double
exactly**, and the same ~0.44% offset at every note is the autocorrelation's own integer-sample-lag
quantization at these short periods, not a real error. **This confirms `note_period()` and Paula's
period register are correct end-to-end for an isolated note** — the earlier Step C round 2 result
only proved the *formula* was exact in isolation (max deviation 0 across all 64 notes); this is the
first time the full `note_on`→audible-PCM pipeline has been checked, and it's also correct.

**This redirects, not resolves, the "pitch off everywhere" worry**: since a real corpus sample's
*audible* pitch equals `freq_hz ÷ (samples in the loop region)`, a `loop_len` that is wrong but
still in-bounds (a value error, not the boundary error §5 fixed) would shift perceived pitch just as
easily — and nothing so far has checked whether `$18`'s resulting *values* (not just their
in-bounds-ness) are what the composer/editor actually intended. That is the open question for next
session, and needs the editor as ground truth (we cannot inspect Paula's real registers without
reading GPL source, and self-consistency checks can't catch a wrong-but-plausible value).

### `$18` ground-truth recipe for next session

**Recipe A (free, try first) — replay the exact already-root-caused case and just listen**:
1. In the editor, open `turrican intro` and play/audition pattern `0x52` (82) — track 3, voice 0,
   macro `0x1c` (28), the exact case §5 root-caused (`docs/macro-playback-fidelity.md` §5's original
   write-up has the full macro-28 disasm and arithmetic).
2. Listen for at least 2-3 seconds: is it a **clean, continuously sustained/looping tone**, or does
   it sound like a **brief blip followed by long silence** (or near-total silence)?
   - A sustained tone corroborates the fix: the new interpretation (`loop_start=39704`,
     `loop_len=128` words = 256 bytes, end `39960`, well inside the 45828-byte `smpl` file).
   - Mostly silence/a click-then-nothing corroborates the *old*, since-replaced interpretation
     (`loop_len=64768` words = 129536 bytes, reading past the file for the overwhelming majority of
     the note).
3. If the editor has any waveform/sample inspector showing the live loop points or playhead, read
   those off directly instead of relying on the ear — a direct numeric comparison beats an audible
   guess. This may not exist; audible discrimination in step 2 is the fallback and is expected to be
   clear enough on its own (fully silent vs. fully audible is not a subtle distinction).

**Recipe B (manufactured, fallback if A's real mix is too noisy/ambiguous)**: build a minimal
macro in the editor mirroring the exact real-corpus numbers for a clean, single-voice comparison —
`SetBegin` into any sample region whose content you know, `SetLen` 1024 words, `DMAon`, then
`Sampleloop` with delta `+1792` (`$00 $07 $00`, i.e. `0x000700`) — copying macro 28's own values
exactly, so the result is directly comparable to the root-caused arithmetic above. Trigger via the
editor's macro-audition feature (same feature used for §2) and listen the same way as step 2 above.

**Bonus, cheap, unrelated to `$18`**: if pitch still sounds "off everywhere" after this is settled,
a complementary check costs nothing extra — pick any pattern note in the editor, note its displayed
note name/transpose, and compare against `tfmx-cli disasm --pattern N`'s decode of the identical
entry. This rules out a decode-level bug (wrong note/transpose *reaching* `note_on`, upstream of
everything this session tested) separately from both the period-math chain (now confirmed correct)


## 9. Recipe A run (2026-08-01): editor corroborates §5; two more real bugs found+fixed on the same
voice, but the user's re-listen still finds voice 0 too low-pitched and pointer-wandering

**Recipe A result**: the user played pattern `0x52`/macro `0x1c` in the editor. Macro `0x1c` loops
cleanly in the editor's own macro-preview, but pattern `0x52` retriggers it every 1-3 jiffies, so
what's audible is a *rhythm* of retriggered notes, not one sustained tone — an important correction
to §8's framing (it assumed a single unbroken tone). Refined discriminator: is there **pitched, tonal
content under the rhythm**, or is it **percussive clicks with no real tone**? The user heard **a
clear tone under the rhythm** — corroborating §5's `loop_len=128`-words (in-bounds) interpretation
over the old out-of-bounds one.

**But `tfmx-cli lint` still flagged voice 0's `sample-region-out-of-bounds` after §5.** Tracing it
(not the loop region — that was fine at `loop_start=39704, loop_len=128`, confirmed correct by the
editor's own audible tone) found two further **real, distinct bugs**, both in `tfmx/src/macro_interp.rs`,
both fixed and TDD'd this session:

1. **`$11 <AddBegin>`'s one-shot form fought the attack→loop handoff.** Macro 28's `$11` opcodes
   (steps 18/21, `±256`) fire *after* `$18 <Sampleloop>` has already handed the voice to loop
   playback, but `MacroInterpreter::render` unconditionally re-pushed the *stale pre-loop*
   `sample_start`/`sample_len` through `Paula::set_sample_region` every jiffy the pointer changed —
   overwriting `Voice::next_sample`'s internal `start == loop_start` handoff (the §4 fix) right back
   to a value the loop math never accounted for, dragging the real read pointer past the 45828-byte
   `smpl` file. Fixed with a `loop_active` flag: once `$18` has run, `$11`'s pointer nudge (both its
   one-shot and periodic-vibrato forms) targets `loop_start`/`loop_len` instead of the frozen
   pre-loop snapshot. Regression test:
   `add_begin_after_sampleloop_wobbles_the_loop_pointer_not_the_stale_attack_one`.
2. **`$05 <Loop>`'s shared `self.repeat` counter got poisoned by the unconditional (`aa=0`) form.**
   Macro 28 nests two counted loops (`aa=5`, i.e. 6 passes each per this crate's off-by-one
   convention) inside an outer *unconditional* loop (`aa=0`, "jump back forever"). All three share
   one `Option<u8>` field. The `times == 0` arm called the same `self.repeat.get_or_insert(times)` as
   the counted arms — on a fresh `None` this inserts `Some(0)` and never clears it before jumping.
   The next *counted* loop instruction reached then finds that stale `Some(0)` instead of `None`,
   reads `left == 0`, and treats itself as already exhausted after **one** pass instead of its own
   six — confirmed by direct instrumentation of `execute()`'s `0x05|0x10` arm against the real
   corpus render: the down-going inner loop (`-256` × should-be-6) ran its full 6 passes exactly
   once, then only 1 pass every cycle after, while the up-going loop (`+256`) kept its full 6 —
   exactly the asymmetric drift the trace showed (`loop_start` oscillating once, then climbing
   monotonically past file bounds). Fixed by never touching `self.repeat` for the `times == 0` case
   (explicit `self.repeat = None` instead of `get_or_insert`). Regression test:
   `unconditional_loop_does_not_poison_a_nested_counted_loops_repeat_state`.

Both fixes: 144 `tfmx` unit tests, full workspace suite, clippy, and `mutation_robustness` all pass.
`turrican intro` voice 0's `sample-region-out-of-bounds` finding is now **fully gone** (was present
before both fixes; the module has zero lint findings related to sample regions afterward). Golden
hashes regenerated for the 4 corpus modules whose audio changed:
`turrican intro`, `apidya (title)`, `turrican 2 level 1-desert`, `turrican 2 title (st)`.

**User re-listen (2026-08-01, full mix + per-voice stems, 90s, `--gate any`) still finds two
problems on voice 0**, both persisting through both fixes above:

- **Pitch still too low** on voice 0's pad, compared to `uade123`.
- **The playhead still audibly moves into sample areas it shouldn't**, as playback of that voice
  progresses — the *symptom* §5/this session's fix #1 targeted is gone from `lint`'s bounds check,
  but something in the same neighborhood is still perceptible by ear.

**Both fixes landed were real and are structurally correct** (verified by targeted regression tests,
not just "stopped triggering the lint rule") — but per this thread's now-repeated pattern (§3/§4/§5
also each "fixed the mechanism, didn't move the ear"), a bounds-respecting fix is not the same as a
*value*-correct one. Two live theories for next session, **not yet tested**:

1. **The `$11 AddBegin` wobble may itself be the wrong mechanism or the wrong magnitude post-loop.**
   `loop_len` for macro 28 is 128 words = 256 bytes; the `$11` wobble this session redirected onto
   `loop_start` swings ±1280 bytes — **five times the loop region's own size**. Even bounds-respecting,
   a wobble that large relative to the loop could plausibly *be* "plays sample areas it's not supposed
   to play" (the loop's read window sliding across five loop-lengths' worth of unrelated sample data)
   and could also explain a pitch shift if it drags the window over lower-frequency content. Cheap
   test: render with `$11`'s post-loop branch temporarily forced to a no-op (skip the add entirely)
   and see whether the "wanders" complaint and/or the pitch complaint changes — isolates whether the
   wobble itself is the culprit before chasing anything else.
2. **`$18`'s resulting loop-length *value* may still be wrong even though now in-bounds** — this is
   §8's original open question, only partly answered by Recipe A (Recipe A confirmed "a tone plays,
   not silence", not "the tone is the right pitch"). A loop region that's longer than the composer
   intended plays back at a lower perceived pitch — consistent with "too low". §8's Recipe A step 3
   (read the editor's own loop-point/playhead inspector directly, if it has one, instead of relying on
   the ear) was never attempted and is the most direct way to settle this: get the editor's actual
   `loop_start`/`loop_len` (or equivalent) for macro 28 and diff against this crate's computed
   `39704`/`128`.
3. **Isolate via `tfmx-cli render-macro`** (bypasses trackstep/pattern/transpose, exactly §8's own
   isolation intent, not yet applied to macro 28 specifically): render macro 28 alone, `measure-pitch`
   it, and compare against the editor's own macro-audition of the same macro (remember §2's `--tempo`
   gotcha — macro-preview timing is unrelated to macro-preview *pitch*, so this comparison is still
   valid despite §2 being about tempo, not pitch).

**Other corpus modules were not touched this session** and still carry `sample-region-out-of-bounds`
findings, unexplored: `apidya (title)` (3), `r-type` (2), `turrican 3 level 1` (2), `turrican 2 level
1-desert` (1), `turrican 2 level 3-flight` (1), `turrican 2 title (st)` (1) — 10 total, up from the
9 recorded after §5 alone (this session's fixes changed *which* instances show up, not just the
count, since the `$05` Loop bug is generic to any macro nesting a counted loop inside an unconditional
one, not specific to macro 28). Whether any of these share either of today's two root causes is
unknown — worth a sweep once voice 0 is fully resolved, but do not assume they're the same bug(s)
without checking.
and `$18`'s loop-length values (still open).


## CHOSEN NEXT STEP (session 9): theory 1 — test whether the `$11` wobble magnitude is the culprit

User chose theory 1 over theory 2 to go next. Not yet executed — this is the exact recipe a fresh
session should run, no re-deriving needed:

1. **Temporarily no-op the post-loop `$11 AddBegin` wobble.** In `tfmx/src/macro_interp.rs`'s `0x11`
   arm (currently around line 710-731, the one-shot branch at `if b1 == 0 { ... }`), change the
   `if self.loop_active { self.loop_start = self.loop_start.wrapping_add_signed(step); }` line to skip
   the add entirely when `self.loop_active` (leave `self.loop_start` untouched post-`$18`; keep the
   pre-loop `else` branch as-is, since that path is outside this experiment's scope). This is a
   throwaway experimental edit — do **not** commit it; revert immediately after rendering (`git diff`
   should be empty again before moving on).
2. **Render two comparison pairs, current tree vs. the no-op'd tree**, using the already-built tooling:
   - `tfmx-cli render-pattern "testdata/mdat.turrican intro" "testdata/smpl.turrican intro" --pattern 82 --seconds 5 -o <before/after>.wav` —
     tightest reproduction (pattern `0x52`/macro `0x1c` alone, preserves the retrigger cadence §10
     showed matters, no full-song noise).
   - Full-mix `tfmx-cli render "testdata/mdat.turrican intro" "testdata/smpl.turrican intro" --seconds 90 --gate any -o <before/after>.wav`
     plus voice-0 stem — what the user actually judges "wanders"/"too low" against.
3. **Quantify before comparing by ear** (per §7's lesson: don't trust "sounds the same" without a
   number first) — reuse §7's own recipe: `sha256sum`, an RMS-of-diff script, `tfmx-cli onset-diff`,
   and `tfmx-cli measure-pitch` on the voice-0 stem pair. A large diff + a measurable pitch shift
   between before/after would corroborate the wobble as (part of) the cause; a negligible diff would
   rule it out cheaply, without needing the user's ears at all.
4. **Only then** ask the user to A/B the full-mix + voice-0-stem pair by ear, the same way §5/§9's
   fixes were verified — per the standing rule, nothing here counts as resolved until they've listened.

If theory 1 comes back negative (no meaningful diff, or diff doesn't move the complaint), theory 2
(get the editor's own loop-point/playhead inspector values for macro 28, §9 above, never attempted)
is the fallback — it's the only remaining way to check whether `$18`'s loop-length *value* itself
(not just its bounds) matches what the composer intended.

---


## 11. Session 9: theory 1 run — mixed result, wobble is real but doesn't move measured pitch

Ran the §9/§10 recipe exactly: temporarily no-op'd the `if self.loop_active { ... }` arm of `0x11`
(`tfmx/src/macro_interp.rs` line ~718) so the post-loop wobble never touches `loop_start`, built,
rendered three before/after pairs (`render-pattern --pattern 82`, full `render --gate any`,
`render --gate any --solo 0`), reverted the edit (`git checkout --`, confirmed `git status` clean
before doing anything else), rebuilt to restore the baseline.

| scope | nonzero-diff | RMS(diff)/RMS(a) | onsets before → after | corr | measure-pitch before → after |
|---|---|---|---|---|---|
| pattern 82 alone, 5s | 43.7% | 113.2% | 20 → 20 | 1.000 | 441.00 Hz → 441.00 Hz (unchanged) |
| full mix, 90s | 35.6% | 46.0% | 150 → 167 | 0.086 | n/a (mixed signal) |
| voice 0 solo, 90s | 30.8% | 60.0% | 127 → 138 | 0.408 | 8820.00 Hz → 8820.00 Hz (unchanged) |

**Not a no-op**: at every scope the diff is large — comparable in magnitude to the confirmed-real
§7 fixes (113%/60%/46% here vs. 250%/187%/205% there) — so the wobble is definitely doing
something audible, not a dead branch.

**But the isolated case (pattern 82, no trackstep/gating interference) shows identical measured
pitch and identical onset timing/rhythm before and after**, despite the large sample-level diff.
That's evidence *against* the wobble being the cause of "too low pitch": in the one scope where
this specific code path's effect isn't confounded by the `AnyTrack` gating cascade (§7's caveat —
a timing shift in one voice can move when its pattern hits `$F0`, which moves the shared trackstep
line, which changes what every voice plays afterward), pitch didn't move at all. The full-mix and
voice-0-solo onset-count/correlation changes are large, but per §7 that pattern (real diff at every
scope, inconsistent direction) is exactly what the gating cascade produces on its own, not
specific evidence for this fix. The voice-0 `measure-pitch` reading (8820 Hz, identical both ways)
is almost certainly not a meaningful note-pitch measurement at that scope — 90s of solo voice 0
includes long silences and multiple different notes, so the autocorrelation is likely locking onto
noise-floor periodicity rather than tracking a single note; the pattern-82 reading (441 Hz, a
single sustained retriggered tone, isolated) is the one to trust.

**Verdict on theory 1: doesn't explain "too low pitch."** The wobble does change sample content
(consistent with contributing to "wanders" — it's still moving `loop_start` by 5-6x the loop
region's size every jiffy, which is a real basis for reading the wrong bytes) but the one clean,
unconfounded measurement available shows no pitch shift. Per the recipe's own threshold (large
diff → worth the user's ears before ruling out entirely) the before/after WAV pairs are worth an
A/B listen on the "wanders" question specifically, but this session did not spend the user's ears
yet — rendered pairs are in the scratchpad, not committed anywhere permanent.


## 12. Session 10: theory 2 — editor decode confirmed correct; `$18` formula independently validated by ear; new SetBegin theory recorded, not yet pursued

The editor has no waveform/loop-point inspector, so theory 2 (§9/§11) ran differently than
planned: the user read macro 28's raw instructions directly off the editor's disassembly instead —
`SetBegin +0x1C14` (step 1), `SetBegin +0x7804` (step 7), `SetLen 0x100`/`0x400`, `Sampleloop
+0x700` (step 0xd).

**Decode cross-check**: `tfmx-cli disasm --macro 28` on `turrican intro` produces the identical
byte-for-byte sequence (`02 00 1C 14`, `02 00 78 04`, `03 00 01 00`, `03 00 04 00`, `18 00 07 00`).
**No decode bug** — this fully answers theory 2's original question (is the macro parsed
correctly?) with "yes."

**Recipe B run (fallback, since the editor has no inspector)**: built a minimal macro in the editor
— `SetBegin` into a known region, `SetLen 0x400` (1024 words), `DMAon`, `Sampleloop` with a
variable delta — and auditioned it directly, sweeping the delta around macro 28's real value
(`0x700`). Result: **looping happens even with `Sampleloop` omitted entirely** (Paula's DMA loops
on the `SetBegin`/`SetLen` region on its own, as `docs/format.md` §8 already describes — `$18` only
*relocates* the loop, it doesn't create looping). Sweeping the delta: **below `0x700`, the loop
sounds "slower"; approaching `0x700`, "faster"; at/above `0x800`, other regions of the sample file
(including silent parts) start playing too.**

This matches this codebase's `$18` formula (`tfmx/src/macro_interp.rs:789-791`,
`loop_len = sample_len.wrapping_sub_signed(delta >> 1) & 0xFFFF`) **exactly, including the precise
threshold**: with `sample_len = 1024` words, `loop_len` hits exactly `0` at `delta = 0x800` (2048,
i.e. `halved = 1024 = sample_len`), and wraps mod 65536 into a huge value past that — which is
exactly why unrelated/silent regions start playing right at `0x800`, not gradually. Below that,
smaller delta → less subtracted from `loop_len` → bigger loop region → slower; delta rising toward
`0x700` → smaller region → faster. The real macro-28 value (`0x700` = 1792) sits well inside the
sane range (`loop_len = 128` words), nowhere near the cliff.

**Conclusion: the `$18` fix's arithmetic is now validated against real editor/hardware behavior,
not just self-consistency** (§5/§6 only checked bounds; this checks the *direction and magnitude*
of the resulting value against an independent ground truth, and the match is exact down to the
breakpoint). This closes out "wrong `$18` loop-length values" as a suspect for the real macro-28
in-song case — the formula is right, and the real delta is nowhere near the degenerate zone.

**New theory recorded, not pursued this session (user's explicit choice)**: `docs/opcodes.md` §3
describes `$02 SetBegin` as adding to "the sample's **base address**" (a fixed noun) versus `$11
AddBegin` adding to "the sample **pointer**" (explicitly the live, oscillating position) — two
different phrases for what might be two different things. The current code (`tfmx/src/
macro_interp.rs:606-613`) implements both by accumulating onto the same `self.sample_start` field,
which makes them behave identically except for `$11`'s extra oscillation — i.e. it reads "base
address" and "pointer" as the same value. Macro 28 has **two** `SetBegin` calls in one trigger
(`+0x1C14` then `+0x7804`), so this is the one case in this investigation where cumulative-vs-
absolute actually diverges: cumulative (current code) gives `37912`; a fixed-base reading could
give something entirely different (e.g. `30724`-based), landing in a different region of the sample
file. This is the strongest remaining lead for "too low pitch"/"wanders" but rests on a single
sentence per opcode with no corroborating passage — needs careful corpus-wide impact analysis and
TDD before touching the code. The existing committed test
`sampleloop_keeps_turrican_intro_macro_28_in_bounds` (`tfmx/src/macro_interp.rs:1607`) pins the
current cumulative reading and would need to change if this theory is ever acted on.

**Still open, untouched this session**: §1 (portamento-to-note drop), the "second, structurally
different out-of-bounds region on `turrican intro` voice 0" from session 4 (separate from the now-
validated `$18` case), and 9 other `sample-region-out-of-bounds` findings across the rest of the
corpus.

**Next**: either (a) get the user to A/B `pattern-before.wav`/`pattern-after.wav` and the
full-mix/voice-0 pairs specifically for the "wanders" (not pitch) symptom, since the numbers here
only rule out a pitch connection, not a wander connection; or (b) move straight to theory 2 (editor
loop-point ground truth for macro 28, §9), which is the only lead left that could explain "too low
pitch" specifically since theory 1's one clean measurement clears the wobble of that symptom.

**Ear result (same session): theory 1 is dead, and not just neutral — disabling the wobble makes
voice 0 sound worse.** The user A/B'd `voice0-before.wav` (wobble on, current/committed code)
against `voice0-after.wav` (wobble no-op'd): *"the pad in voice 0 before sounded better, having
some kind of modulation sweep. the voice 0 after version sounds like the loop is looping over a
smaller section, thus sounding faster and wrong/grainy."* So the wobble is not an unwanted
"wandering" artifact — it's what gives the pad its sweep/chorus character, and removing it exposes
a small, static loop region that sounds thin and grainy instead. **Do not remove or weaken this
code path.** This also reframes "wanders" from the original complaint: it was never about this
`$11` post-loop wobble specifically (which the user now confirms sounds *right*), so whatever
originally prompted "the playhead moves to the wrong places" is still unidentified.

New data point worth carrying into theory 2: the no-wobble loop alone (`loop_start`/`loop_len` as
`$18` computes them, no per-jiffy movement) reads as "smaller than it should be" by ear — faster
and grainier than the real instrument. That's a plausible base for "too low pitch" being wrong in
the *other* direction than assumed (a loop that's too short raises pitch, doesn't lower it) — worth
explicitly asking the editor's loop-point inspector whether `loop_len=128` words (macro 28,
`turrican intro`) matches, undershoots, or overshoots the composer's intended region, not just
whether some other value is "more correct" in the abstract.


## 13. Session 11: `$02 SetBegin` made absolute per docs — real corpus fix, does not move the ear complaint

Acted on §12's recorded theory. `docs/playback-model.md` §2's own table already states `$02
SetBegin` targets an "absolute smpl offset" — unlike `$18 Sampleloop`'s explicitly-documented
additive, non-idempotent delta (§7 gotchas, its own doc comment in `tfmx/src/macro_interp.rs`).
The code contradicted its own project's docs: `self.sample_start = self.sample_start
.wrapping_add_signed(delta)` accumulated every `$02` within a trigger onto the previous one.

**Corpus-wide scan before touching code** (literal-listing `$02` count per macro via `tfmx-cli
disasm`, all 128 macros × 10 modules): 60+ macros across 6 of 10 modules issue `$02` two or three
times per trigger, not just macro 28. `turrican 2 level 1-desert` macro 33 issues it three times,
each followed by its own `SetLen`/`Wait`/`$19 <Set one shot sample>` — three independent one-shot
samples played back to back, which only makes sense if each `$02` is absolute; cumulative
addressing would walk every sample after the first arbitrarily far from any real data.

**Fix**: `tfmx/src/macro_interp.rs`'s `0x02` arm now sets `self.sample_start` directly from the
24-bit operand instead of adding it to the running value. `$11 AddBegin` and `$18 Sampleloop` are
untouched — both already have their own explicit "this one is additive" doc language, so the
absolute reading applies to `$02` only. TDD: `set_begin_is_absolute_not_cumulative` added; the
existing `sampleloop_keeps_turrican_intro_macro_28_in_bounds` test's expected `loop_start` updated
(37912 cumulative → 30724 absolute, the macro's *second* `$02` value winning outright).

**Structural result, measured before asking for ears**: `tfmx-cli lint`'s `sample-region-out-of-
bounds` findings across the corpus dropped from 9 (6 modules) to 3 (all in `apidya (title)`, the
already out-of-scope TFMX 7V module) — every other affected module's out-of-bounds reads are now
gone. 145 `tfmx` unit tests, full workspace suite, `mutation_robustness`, clippy all pass. 4 of 10
modules' golden hashes changed within the first 10s (`apidya (level 1)`, `r-type`, `turrican 2
level 3-flight`, `turrican 2 title (st)`); `turrican intro`'s did not (macro 28's pattern-0x52
usage falls later than 10s), regenerated.

**Isolated pattern-82 measurement** (no trackstep/gating confound, same recipe as §11): rhythm
unchanged (20→20 onsets, correlation 1.000) but measured pitch jumped **441 Hz → 8820 Hz**, a 20x
change, moving in the "was too low" direction but by an implausibly large amount for a single
semitone-scale correction.

**Ear result: negative.** The user A/B'd `voice0-before.wav`/`voice0-after.wav` and the full mix:
**"it does not sound pitch correct (and overall the other voices also do not sound pitch correct).
and the wandering also does not seem to be fixed."** So this fix, like §7/§9's, is real and
independently justified (matches the project's own docs, fixes 6 real out-of-bounds bugs) but is
not the explanation for the standing "too low pitch"/"wanders" complaint — and the fact that
*other, unrelated voices* also sound pitch-wrong is new information: it reframes the complaint as
not specific to macro 28's `$02` handling, or possibly not specific to any single opcode at all.
**Kept and committed anyway** (user's explicit call) since it stands on its own evidence
independent of this thread's main complaint.

**Next**: the user is pivoting the diagnostic strategy — rather than continuing to chase individual
opcodes inside real, complex corpus macros, build a minimal from-scratch test song: a handful of
trackstep lines on one track playing one pattern, which plays a musical scale through the simplest
possible macro (a single non-looping or simply-looping sound, e.g. a synthesized sine wave or one
lifted from `smpl.turrican intro`). Isolates pitch from every remaining confound (real macros'
multi-stage envelopes, retrigger timing, the `AnyTrack` gating cascade) by construction rather than
by after-the-fact measurement.


## 14. Session 11 continued: minimal from-scratch test song built, A/B'd against the real editor — likely root cause found, a `MIDDLE_C_NOTE` anchor off by a tritone

**Built `testdata/synth/`** (`gen_minimal_scale.py`, tracked in git — synthesized, not copyrighted,
unlike the rest of `testdata/`, which stays gitignored): a from-scratch `mdat.minimal-scale`/
`smpl.minimal-scale` pair with one trackstep line, one pattern playing a 13-note chromatic scale
(notes `$1E`–`$2A`, one octave), through the simplest possible DMA-on macro (`$00,$02,$03,$08,$01,
$07` — set region, set pitch from the note, DMA on, stop; Paula loops the `SetBegin`/`SetLen`
region on its own, no `$18` needed), over a synthesized 32-sample sine cycle (chosen so note 30
lands near real middle C, ~261 Hz). Isolates pitch from every real-corpus confound by construction.

**Iteration 1** (tone at `smpl` offset 0, tempo 0): silent in the real TFMX editor, despite
measuring correctly through this crate's own renderer. Two candidate causes fixed without proof
either was *the* cause, both independently justified: `docs/format.md` §8 notes both real `smpl.*`
files begin with 4 zero bytes ("suggesting offset `$0` conventionally holds a short silent null
sample") — moved the tone to offset 4 and reserved offset 0 as silence; tempo 0 had never actually
been validated in the editor (every prior editor tempo test used 2 or 3, `docs/trackstep-
timing-bug.md` §3) — changed to tempo 3. **Iteration 2 was audible in the editor.**

**A/B result: pitch-correct in isolation (this crate's own renderer, all 13 notes within ~1% of
predicted), but the editor plays it at a consistently higher pitch than this crate does, for
identical note bytes.** Precisely quantified via the user's own controlled experiment in the
editor: changing the pattern's first note from `$1E` ("C-3") to `$18` ("F#2") made the editor's
output match this crate's rendering of the *original* `$1E` note. Confirmed at the octave's other
end too (`$2A`→`$24` behaves the same way). Both pairs are exactly six semitones (a tritone, factor
`2^(6/12)≈1.414`) apart — in an equal-tempered system, only a constant note-index (or equivalently
a constant frequency-multiplier) offset reproduces *two* such pairs exactly, so this reads as a
uniform, note-independent error, not a per-note or per-range one.

**Primary-source check**: re-fetched `[S1]` itself (J. H. Pickard, *TFMX Professional 2.0 Song File
Format*, freely available e.g. via `libxmp`'s bundled format docs) to check for a transcription
error in `docs/playback-model.md`'s citation. None found — the quote is verbatim: *"All notes are
based at `$1E`=middle C (8363Hz).."*, following the same F#0/G-0/.../C-3/.../C-4 note-name table
already in `docs/playback-model.md` §4. Critically, **`[S1]` gives only that one anchor point and a
name table — it never states the note→frequency *formula*** (`docs/playback-model.md` line 407
already flags the equal-tempered exponential formula as "inferred," not `[S1]`-stated). That gap is
exactly where a wrong anchor could hide: every prior validation of `note_period()` in this
investigation (the semitone-ratio sweep, the octave-doubling invariant, the three-octave-apart
`measure-pitch` readings in the earlier `note_period()` round) checked the formula only *against
itself* — a uniform wrong anchor constant preserves every ratio and every octave-doubling
relationship perfectly while still being globally wrong, so none of those checks could ever have
caught this. This session is the first time pitch has been checked against real hardware/editor
ground truth at all.

**Mixer ruled out as the fix location**: checked `tfmx/src/paula.rs`'s `Voice::next_sample` (the
period→playback-rate resampling math) for a hidden constant-factor bug independent of
`note_period()` — `freq_hz = PAULA_CLOCK_HZ / period; step = (freq_hz/sample_rate) * 2^32` is
standard, clean resampling arithmetic with no extra factor found. The likely fix is localized to
`tfmx/src/macro_interp.rs`'s `const MIDDLE_C_NOTE: i32 = 0x1E;`, which the evidence says should be
`0x18` instead (keeping `MIDDLE_C_HZ = 8363.0` as-is — `[S1]`'s prose ties that number to the wrong
index, but the number itself is a well-known standard Amiga constant, independently plausible).

**Not yet done, chosen next step for a fresh session**: implement the `MIDDLE_C_NOTE` change
(`0x1E` → `0x18`), TDD'd. This is foundational and wide-blast-radius — it shifts *every* note in
*every* module by a tritone, so before calling it done:
- Update every hard-coded worked example tied to the old anchor: `docs/playback-model.md` §4's
  `period(0x1E)=424`/`freq(0x1E)=8363Hz` examples, and any `tfmx` unit test asserting an absolute
  period/frequency value for a specific note (the octave-doubling/semitone-sweep tests are relative
  and should keep passing unchanged; anything anchored to a literal note number's *absolute* period
  will need its expected value recomputed against the new anchor).
- Regenerate golden hashes for **all ten** corpus modules (this is not a narrow, module-specific
  change like every previous fix in this thread — every rendered note shifts).
- Re-render the `testdata/synth/mdat.minimal-scale` scale and re-verify against the editor's own
  playback directly (the control this session already established) before trusting the corpus-wide
  regeneration.
- Per the standing rule, get the user's ears on a real corpus module (e.g. `turrican intro`) A/B'd
  against `uade123` again — this is the first fix in the whole thread with a real *mechanism*
  matching the "too low pitch, every voice" shape of the original complaint (a per-opcode data bug
  would be spotty across voices/modules; a wrong global pitch anchor is not), so it is worth
  treating as a serious root-cause candidate, but per this thread's own repeated lesson (`§7`, `§9`,
  `§13`), do not declare it fixed until confirmed by ear.
- `testdata/synth/gen_minimal_scale.py` is the fast control loop for this: regenerate, render, and
  A/B against the editor again after the code change, before spending corpus-wide listening time.

**Update (2026-08-01, session 12): `MIDDLE_C_NOTE` changed `0x1E` -> `0x18`, TDD'd, not yet heard.**
Implemented exactly the change §14 proposed, nothing more: `tfmx/src/macro_interp.rs`'s
`MIDDLE_C_NOTE` constant, doc comment updated to record the editor A/B rather than cite `[S1]`'s
(now-falsified) anchor claim. Five tests hard-coded an absolute period at note `$1E` (the old
anchor) — updated to `$18` (renumbering the note bytes each test triggers, not just the literal
expected period, so each test still exercises "the anchor note") — plus two worked-example
literals (`middle_c_matches_the_worked_example`, `one_octave_up_halves_the_period`) and their
octave-up counterpart moved `$2A` -> `$24` to stay one octave above the new anchor. The
octave-doubling and independently-walked-semitone-ratio sweep tests needed no change (self-
referential against `MIDDLE_C_NOTE`, not a hard-coded note). `docs/playback-model.md` §4's worked
examples and prose updated to the `$18` anchor, with an explicit note that `$1E` keeps its table
*name* ("C-3") — only the frequency anchor moved. `tfmx-cli`'s `measure-pitch` help text
(`note-30` -> `note-24`) updated too. 145 `tfmx` unit tests, full workspace suite (`tfmx-cli`'s 59
+ golden + doctest), `mutation_robustness` and clippy all pass. Golden hashes regenerated for
**all ten** corpus modules, as expected (every note's pitch shifts by this fix, unlike every
earlier narrower fix in this thread). Rendered fresh WAVs for the user's A/B, not yet listened to:
`testdata/synth/mdat.minimal-scale` re-rendered (session scratchpad, not committed — the
generator script's own header comment about which note lands near "real middle C" is now stale
prose, harmless since it doesn't drive the actual test data), and a full-mix + 4 stems of
`turrican intro` (`--seconds 90 --gate any`, session scratchpad). **Per this thread's own repeated
lesson (§7/§9/§13): this is not done until the user's ears confirm it against the real editor
(the minimal scale) and ideally `uade123` (the corpus module) — do not upgrade this past "likely"
without that.**

**Ear result (same session): pitch confirmed correct on the full mix.** The user listened to the
`turrican intro` full-mix render and reports the pitch now sounds right, as far as they can tell.
`MIDDLE_C_NOTE` (§14, commit `e17b5e3`) can be treated as resolving the pitch complaint.

---

