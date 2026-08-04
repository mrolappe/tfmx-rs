# Macro/pattern fidelity: Paula attack→loop handoff undone every jiffy, + differential-render validation method

**Status: FIXED, TDD'd**, and independently confirmed non-inert via a three-scope differential render (§7 below) — a reusable recipe for "did this fix actually change anything" used throughout the rest of this investigation.

[← index](macro-playback-fidelity.md)

---

## 4. CONFIRMED BUG, fixed 2026-08-01: `Paula::set_sample_region` undid the attack->loop handoff every jiffy

Found while chasing the user's post-fix listen of §3: pitch still off, macro `0x0a`'s playback
still wrong with "the playhead pointing into a wrong region of the sample" later on, and
pattern `0x54`'s macros `0x30`/`0x31` on voice 2 sounding too slow. This is a second, unrelated
bug, structural and much bigger in blast radius than §3 -- it affects every voice using the
one-shot-attack-then-loop pattern, not just macro `0x0a`.

`MacroInterpreter::tick()` calls `paula.set_sample_region(voice, sample_start, sample_len)`
**every single jiffy**, using its own `self.sample_start`/`self.sample_len` fields -- including
jiffies where the macro program made no change to them at all (e.g. sitting inside a `$04
<Wait>`). But `Voice::next_sample` (`tfmx/src/paula.rs:49-80`) silently advances `Voice::start`/
`len` from the one-shot attack region to `loop_start`/`loop_len` once the attack region is
exhausted -- a deliberate shortcut this crate uses instead of relying on tick-accurate `$18
Sampleloop` register rewrites (see its own doc comment). `set_sample_region` re-applied its
unconditional `start`/`len` write every jiffy regardless, so the very next jiffy after the
handoff silently undid it, snapping playback back to the attack region while `frac` (never
reset by this call) kept counting up unbounded against whatever length happened to be active --
corrupting the effective read position more and more as playback continued, which fits every
symptom reported: wrong pitch (reading the wrong region entirely), the sample literally reading
from the wrong place later in playback, and cadence effects built on `loop_completions`
(`$1A <Wait on DMA>`) skewing since completions were being mis-counted against a length that
kept getting reset.

**Test seam gap, same pattern as §3**: the existing `render_transitions_from_attack_to_loop_region`
test (`tfmx/src/paula.rs`) only calls `set_sample_region` **once**, then renders in a single
continuous span -- it never re-asserts the region across multiple ticks the way
`MacroInterpreter::tick()` actually does, so it could never have caught this.

**Fixed, TDD'd**: `Voice` gained a `requested_region: Option<(u32, u32)>` field recording the
last region a caller explicitly asked for, distinct from `start`/`len` which the mixer may have
since silently advanced. `set_sample_region` now only touches `start`/`len` when the requested
region actually changes, letting an in-progress attack->loop handoff persist across identical
same-tick re-broadcasts. To avoid a matching edge case -- a genuine retrigger that happens to
re-request the *same* `(start, len)` as before, after the mixer had already drifted away from it
-- `set_dma`'s existing off->on edge (which already resets `frac` on a retrigger) now also force-
resyncs `start`/`len` from `requested_region` unconditionally. New test
`repeated_identical_set_sample_region_calls_do_not_undo_the_loop_handoff` pins the bug (confirmed
red beforehand: it read back the attack region's value forever instead of ever settling into the
loop region); all 140 `tfmx` unit tests and `mutation_robustness` still pass. Re-traced `turrican
intro` voice 0 over 20s post-fix: the voice now genuinely settles into and stays in a loop
region across dozens of consecutive jiffies (e.g. one region persists for 42 rows straight)
instead of reverting every tick.

**Not yet heard** -- this is a bigger, more fundamental fix than §3 and touches every voice in
every corpus module that uses the one-shot/loop pattern (most of them), so the next listen should
cover more than just `turrican intro` pattern `0x6b`/`0x54` once the user has time.

---

---


## 7. RESOLVED, 2026-08-01: the §3/§4 "no audible difference" disconnect explained (not a no-op)

**Method**: isolate the §3/§4 diff (`git stash push -- tfmx/src/macro_interp.rs tfmx/src/paula.rs`,
which at the time held exactly and only those two fixes, nothing else) to build a "pre" binary,
render, `git stash pop` to restore the "post" (current working-tree) state, rebuild, render again,
`git stash pop`/rebuild always restores the tree before doing anything else. Compared at three
scopes, pre vs. post:

| scope | command | nonzero-diff samples | RMS(diff)/RMS(a) | onsets a → b |
|---|---|---|---|---|
| macro 10 alone, 5s (`render-macro --macro 10`) | isolates exactly §3's fix target | 18.4% | 250% | 15 → 14 |
| pattern `0x6b`/107 alone, 10s (`render-pattern --pattern 107`) — the pattern the user keeps A/B-ing for §1 | one level up: a real note sequence driving macro 10 (and others) | 93.5% | 187% | 17 → 26 |
| full song, 60s (`render --song 0`) | what the user actually re-listened to | 43.0% | 205% | 258 → 156 |

**Finding**: at every scope, the rendered PCM differs enormously — RMS of the pre/post *difference*
exceeds the RMS of the signal itself, and even the smallest, most isolated case (one macro, no
pattern or trackstep context) has audible-scale change. This rules out "the fix is a no-op that
happens to net out to nothing" and rules out a pure downstream-cascade explanation too (the single
*macro* case alone already shows large change, before any pattern or multi-track gating is
involved). The disconnect is therefore not "nothing changed" — the user's "sounds more or less the
same" verdict was evidently about persistent *character* (pitch still off, general quality still
wrong), not about the waveform being unchanged. That character most likely comes from bugs §3/§4
don't touch: §1 (portamento silently dropped, on this exact pattern 107) and §5 (out-of-bounds
silence) are still unfixed in every render above, pre and post. Note the *direction* of the onset
change isn't even consistent across scopes (pattern 107 gains onsets, the full song loses nearly
40% of them) — another sign this is real, structural, note-selection-level change (very plausibly
via `AnyTrack` trackstep gating: a shifted per-jiffy timing in one track's macro can move when that
track's pattern hits `$F0`, which moves the whole line, which changes what every other track plays
from that point on) rather than a subtle timbral nudge.

**Why isolate at three scopes, not just re-listen to the full song**: `render-macro` and
`render-pattern` turned a subjective "does the fix help" question into a falsifiable one at
shrinking blast radius:
- `render-macro` pins a diff to *exactly* the code path a fix targets, with zero interference from
  the other 3 voices, the pattern's own note sequencing, or trackstep gating — the tightest
  possible reproduction of "did this specific fix change this specific macro's output."
- `render-pattern` adds back a real note sequence and multi-macro interaction but still removes
  `Sequencer`/trackstep, so it isolates one pattern's authored behavior from the `AnyTrack` gating
  that couples every track's timing to every other's.
- The full-song render is the only one that matches what a listener actually hears, but it's the
  worst tool for attributing *why* something changed — too many simultaneous macros, patterns and
  the gating cascade above to point at one cause.
Comparing magnitude and direction across all three, rather than trusting any single one, is what
surfaced the gating-cascade hypothesis and ruled out "the fix is inert." The same recipe (stash the
one change under test, build, render at macro/pattern/song scope, `sha256sum` + RMS-of-diff +
`onset-diff`) is reusable for any future fix where "does this actually change anything" needs an
answer more precise than "sounds about the same."

**Not yet done**: no numeric metric here directly measures "wrongness" (pitch drift, missing
glide) the way the user's ear does — RMS-of-diff and onset count only prove *something* structural
changed, not *what*. Confirming §1/§5 are the dominant residual cause still needs either fixing them
and re-listening, or a targeted metric (e.g. tracking `note_period()` drift over pattern 107's
known portamento step).

---

