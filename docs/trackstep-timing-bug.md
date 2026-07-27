# The trackstep timing bug

Findings from 2026-07-27 on the long-running "different melody" complaint against
`turrican intro`. Two separate defects in how the shared trackstep line pointer is
driven. The first is found, fixed and tested; the second is measured but not yet
resolved. Neither has been confirmed by ear.

This document exists because the investigation that found these had spent several
sessions in the wrong layer, and the reasons why are as reusable as the findings.

---

## 1. The line pointer advanced on the clock, not on `$F0 <End>`

### What the sources say

[`opcodes.md`](opcodes.md) §2, at **documented** confidence — the strongest label in
that table:

> | `$F0` | `<End>` | `xx xx xx` | Ends this pattern; **trackstep advances**. | documented |

The one thing no source states is how the *eight* tracks aggregate into the *one*
shared line pointer. [`playback-model.md`](playback-model.md) §7 already recorded that
gap:

> What actually triggers a trackstep line advance: [S1] ties it to a pattern's
> `$F0 <End>`, but does not say whether *every* active track's pattern must reach it
> before the shared line pointer moves, or whether any one track's `$F0` is enough.

### What the code did

`Player::run_jiffy` called `Sequencer::advance()` **unconditionally, every tick**. Its
comment claimed to have resolved §7's open question:

> Resolves a previously-open question (`docs/playback-model.md` §7) … This crate's
> reading is the latter: `docs/opcodes.md` §1's per-track word table exists precisely
> so that "hold the current pattern, just update transpose" (`$80`) can be the common
> case across many consecutive jiffies … it is authored data (mostly `$80 Hold` words)
> that makes most of those evaluations a no-op, not a gating condition on pattern
> completion.

### The premise is false

Counting `$80 Hold` against explicit `Pattern` words across the trackstep data of every
corpus module's song 0, alongside how long each referenced pattern actually runs before
reaching `$F0`:

| module | `Pattern` words | `Hold` words | Hold % | patterns | mean ticks to `$F0` |
|---|---:|---:|---:|---:|---:|
| turrican intro | 1760 | **0** | **0.0 %** | 52 | 55.6 |
| r-type | 908 | **0** | **0.0 %** | 37 | 29.8 |
| turrican 2 level 1-desert | 959 | **0** | **0.0 %** | 47 | 31.4 |
| turrican outside | 744 | **0** | **0.0 %** | 29 | 38.9 |
| x-out (title) | 105 | **0** | **0.0 %** | 24 | 80.4 |
| apidya (level 1) | 850 | 150 | 15.0 % | 9 | 50.7 |
| turrican 2 level 3-flight | 1627 | 188 | 10.4 % | 32 | 139.8 |
| turrican 2 title (st) | 1462 | 188 | 11.4 % | 61 | 83.0 |
| apidya (title) | 4875 | 675 | 12.2 % | 43 | 332.5 |
| turrican 3 level 1 | 3786 | 2483 | 39.6 % | 28 | 458.4 |

**Five of ten modules contain zero `$80 Hold` words**, `turrican intro` among them.
Every line is a full re-assignment. Meanwhile the average pattern is tens to hundreds
of ticks long and was given **exactly one tick** before its track was reassigned and
its runner restarted at step 0.

For `turrican intro` that is roughly **98 % of the composed music never playing** —
each pattern's first step or two, then on to the next arrangement line. Correct
instruments, plausible note density, entirely the wrong tune.

### Corroboration

- `turrican intro`'s **pattern 21 is, in its entirety, `Wait(31); End`** — 32 ticks of
  nothing. Meaningless filler under a clock-driven advance; under `$F0` gating it is
  how a track is padded (or a line is timed) to a musical length.
- All 52 patterns song 0 references terminate: **39 with `$F0 End`, 13 with `$F4 STOP`**.
  Nothing runs off its end, so gating on `$F0` is well-founded in the data.
- `$F4 <STOP>` is documented as *"unrecoverable until a new pattern pointer is loaded;
  **will not run any upcoming `<End>`**"* — a track sitting on one can never cast a
  vote, which is exactly the opt-out an all-tracks rule needs to avoid stalling.
- Song 0 looped after **4.4 seconds**. For the Turrican intro.

### What this also explains

Several findings from earlier sessions were symptoms of this, not independent problems:

- the **71 % superseded-trigger rate** (144 of 202 genuine triggers never reached their
  own `$01 DMAon`) — tracks were being reassigned and reset every single tick;
- the recurring **all-voice silence gaps**, since `$00 <DMAoff+Reset>` ran on every
  track every tick;
- `lint`'s **`no-retrigger`** finding, and the "fast note run retriggering the same
  macro every jiffy" that the `note_on` fix worked around rather than cured;
- the 2-jiffy attack lag being "systemic with zero exceptions" — true, and unavoidable
  when nothing is ever given more than one tick.

### The fix

`TrackstepGate::{AllTracks, AnyTrack}` and `trackstep_line_due()` in
[`../tfmx/src/player.rs`](../tfmx/src/player.rs). A track holds the line only while it
is running a pattern that has not yet reached `$F0`; `$F4`/`$FE` drop out of the vote;
with nobody holding it the line advances, which is also what boots the first line and
what carries `$EFFE` command lines.

Both readings of §7's open question are selectable — `Player::set_trackstep_gate`, and
`tfmx-cli render|trace --gate all|any` — because no published source settles it.

Track words are now applied **only on the jiffy their line is consumed**, and an
explicit `Pattern` word restarts the runner while `$80 <Hold>` deliberately does not
("keep the currently running pattern; only the transpose changes"). That is the
documented semantics stated directly, and it let the `track_pattern` shadow field —
which existed only to tell a Hold-resolved slot from a real assignment — be deleted
along with the `$FB <PPat>` interaction it was guarding.

Five tests in `player.rs`, mutation-checked twice: all five fail under an
always-advance mutation (the old behaviour) and all five fail under an over-strict one
that deadlocks when no track is running.

---

## 2. A second, coupled defect: the tick rate

With gating in place `turrican intro` becomes far *too slow* — 4 to 14 minutes for a
song that should be around a minute. So the tick rate is wrong as well, and the two
defects were masking each other: a line pointer running ~50× too fast on a clock
running ~4× too slow.

### The measurement

Autocorrelating the RMS envelope (20 ms hop) of a 180 s `uade123` reference render of
`turrican intro` song 0, the six strongest periods are:

| period | × 0.64 s | jiffies at 50 Hz |
|---:|---:|---:|
| 5.12 s | 8.00 | 256 |
| 5.76 s | 9.00 | 288 |
| 7.68 s | 12.00 | 384 |
| 10.24 s | 16.00 | 512 |
| 30.72 s | 48.00 | 1536 |
| 89.60 s | 140.00 | 4480 |

**Every one is an exact integer multiple of 0.64 s = 32 jiffies at 50 Hz**, with no
rounding slop — and pattern 21 is exactly 32 ticks long. The reference's musical grid
is 32 jiffies at 50 Hz.

### Why that contradicts the current model

Song 0's stored tempo is genuinely 3. The header tables are correctly aligned —
verified directly against the raw bytes: `start[0]=75`, `end[0]=129`, `tempo[0]=3`, and
`tfmx-cli info` agrees. [`playback-model.md`](playback-model.md) §3.2's cited rule
(`v ≤ 15` → `tick_rate_hz = 50 / (v + 1)`, with [S1]'s own worked value "2 = 16.7 Hz")
turns that into **12.5 Hz**, which cannot produce a 0.64 s grid.

Note also that songs 1 and 2 store tempo 120 and 160 — BPM-path values giving 48 Hz and
64 Hz, both plausible jiffy rates. Song 0's 3 is the odd one out.

### Weak early signal on the gate variant

A throwaway build with the tick rate forced to 50 Hz (reverted; never committed)
rendered:

- `--gate any` — a razor-sharp loop, **correlation 0.948 at 57.38 s**;
- `--gate all` — no clean loop at all, ~0.50 smeared across many lags.

That leans towards `AnyTrack`, but it is one module from a hacked build, so the default
stays `AllTracks` until the editor or a listen says otherwise.

---

## 3. Open questions

Both are for the TFMX editor, which loads the corpus, displays the data symbolically
and plays it — the only reference that can answer either by inspection:

1. **Does a trackstep line end when *every* active track reaches `$F0 <End>`, or when
   *any one* does?** (`playback-model.md` §7.) `--gate all|any` renders both.
2. **How does the tempo value relate to pattern `Wait` counts?** Read the editor's own
   tempo field for `turrican intro` song 0 and time one trackstep line against it.

---

## 4. Method notes

Worth keeping, because the delay in finding this was a method failure rather than a
hard problem.

- **A comment that claims to close an open question is not evidence.** `run_jiffy`'s
  said "Resolves a previously-open question … This crate's reading is the latter" and
  gave a plausible reason. It was a guess, and the corpus falsifies its stated premise
  in a single count. Treat every such comment as unverified until data confirms it, and
  prefer wording that says *chosen*, not *resolved*.
- **Read the layers you have not read.** Every prior round attacked this through audio
  (spectrograms, RMS onset detection, autocorrelation pitch matching) or through the
  macro/pitch layer, because the inherited record framed the residue as "note/pitch/
  instrument dispatch". Printing `tfmx-cli trace` beside `tfmx-cli disasm --pattern`
  and noticing that pattern 84's `Wait(31)` never had time to run took five minutes.
- **Implausible numbers in the record are leads.** "Loops at ~4.4 s" was carried across
  several sessions as a fact about `turrican intro`. It should have been an alarm.
- **Structural checks beat audio comparison for structural bugs.** The Hold count and
  the mean-ticks-to-`$F0` column are two `grep`-level statistics over data the crate
  already prints, and either one alone falsifies the model. No DSP required.
- **Autocorrelating the reference's envelope is a reusable measurement.** Unlike the
  onset-rate method (structurally blind in dense polyphony) and the pitch-matching one
  (locks onto harmonics of short wavetable samples), it recovers the arrangement's own
  grid, which is exactly what a timing bug distorts. Both earlier methods are recorded
  as unreliable elsewhere in this repo; this one held up.
