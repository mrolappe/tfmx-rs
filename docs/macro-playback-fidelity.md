# Macro/pattern playback fidelity: open items from a live editor cross-check

Started 2026-07-31, with the trackstep-timing fix (`docs/trackstep-timing-bug.md`) code-complete
but not yet confirmed by ear. While preparing for that listen, the user cross-checked individual
macros against the TFMX editor's own macro-audition feature and found real discrepancies,
independent of the trackstep-timing work. This document exists so a fresh session can pick the
investigation up without re-deriving the tooling or the findings below.

**▶ START HERE (2026-08-04): next session's tasks, in order:**
1. This document has grown to 18 sections spanning many distinct bugs across several corpus
   modules. Split it: one document per distinct issue (e.g. §1 portamento-drop, §16 `$0B` direction,
   §18 pattern-82 note durations, etc.), cross-linked from here rather than inlined. Keep this file
   as an index/overview once split.
2. Commit the split documents.
3. Report current context usage, then summarize the two live options for §18's open question
   (editor ground truth for pattern 82, vs. rechecking `$00`/`$08`'s real suspend timing —
   see §18's "Open question" and "For whoever picks this up next") and ask the user which to
   pursue before doing either.

**Status (2026-08-01): §3 and §4's fixes are in, TDD'd, and structurally verified. The "unresolved
disconnect" (user's ears said the audible symptoms were unchanged) is now explained, not
resolved — §7: a differential render proves the fixes are far from inert (up to ~93% of samples
differ, RMS of the diff exceeds the RMS of the original signal), so "sounds the same" was a
statement about *character* (still pitch-off, still wrong), not about the waveform being
unchanged. §1 (portamento drop) and §5 (out-of-bounds silence) remain the leading suspects for
that persistent character, and are next.
§5 was a newly found, math-confirmed silence bug (pattern `0x52`/macro `0x1c`/voice 0); a
corpus-wide sweep (2026-08-01, a `sample-region-out-of-bounds` lint finding) showed it wasn't an
isolated case (26 voice-instances across all ten modules). Its root cause — `$18 Sampleloop`
subtracting a byte-valued delta from the word-valued `loop_len` with no unit conversion — was
FIXED and TDD'd (2026-08-01, drops the corpus-wide count to 9), triggered by a fresh user listen
that named voice 0 and voice 2 exactly, matching the lint findings on the nose. **Re-listened: NOT
audibly different either** — same disconnect as §7. **§8 (2026-08-01): a from-scratch isolated test
(new `tfmx-cli measure-pitch` tool) confirms `note_period()`+Paula's period register are correct
end-to-end for a clean note, redirecting suspicion onto whether `$18`'s resulting loop-length
*values* (not just their in-bounds-ness) are actually correct — needs the editor as ground truth,
concrete recipe at the bottom of §8, chosen next step.**
§1 still awaits a fix decision, §2 is now resolved (editor's macro-audition previews at the fastest
jiffy rate, not the song tempo). §6 and **§10 (2026-08-01, session 8)** are resolved *tooling*
gotchas (not engine bugs) found while using the new `render-pattern`/`render-macro` isolation
commands — read them before re-reporting any "render-macro produces silence" symptom, and before
trusting a `measure-pitch` reading on a macro that depends on its pattern's own retriggering to stay
audible (§10: macro 28 alone goes silent after ~60ms via `render-macro`; `render-pattern` doesn't
have this problem). §9's theories 1 and 2 remain the live next steps.**
**§16 (2026-08-04, new session): `$0B <Portamento>`'s whole documented model (open-ended
`period *= (256+bb)/256` forever, `bb`'s sign alone deciding direction) is now contradicted by
direct editor evidence — real behavior needs a following `AddNote`/`SetNote` to do anything at
all, and direction depends on more than just `bb`'s sign. Root cause NOT found — recorded for a
fresh session, unrelated to §15's still-paused clock-domain thread.**
**§18 (2026-08-04): `turrican intro` pattern `0x52`(82)/macro `0x1c`(28) has wrong note
durations — root cause found (a race in `note_on`'s `dma_on`-based sustain heuristic), ear-confirmed
by the user, fix NOT yet chosen.** This is the same pattern/macro §5-§12 already root-caused for
silence/out-of-bounds; those are fixed, but a distinct bug in the same neighborhood survived: some of
the pattern's 5 notes per cycle never get a real restart. Root cause and fix candidates below —
touches the same `note_on` heuristic three other now-fixed bugs depend on, so needs care, not a
quick guess.

---

## 1. CONFIRMED BUG: portamento-to-note pattern records are silently dropped

**This is the headline finding — high confidence, pinned to one line.**

`docs/format.md` §6 / `docs/opcodes.md` §2: a pattern note longword is `aa bb cv dd`. When
`aa > $BF` (i.e. `$C0`-`$EF`), "the note is reached by portamento from the previous note ... rather
than played directly." The crate decodes this correctly as `NoteTiming::Portamento(dd)`
(`tfmx/src/sequencer.rs:530`, `dd` presumably a portamento rate/speed, same idea as `$FC <Port>`).

But `dispatch_pattern_entry` (`tfmx/src/player.rs:382-420`) never uses it. Every `NoteTiming`
variant — `Detune`, `Wait`, **and `Portamento`** — is routed through the exact same call:

```rust
// tfmx/src/player.rs:407-411
let detune = match timing {
    NoteTiming::Detune(detune) => detune,
    NoteTiming::Wait(_) | NoteTiming::Portamento(_) => 0,
};
macros[voice as usize].note_on(macro_number, note, volume, transpose, detune);
```

The `Portamento(dd)` payload is matched only to discard it as "no detune" — `dd` (the portamento
rate) never reaches `MacroInterpreter::start_portamento` or anywhere else. The result: a
portamento-to-note entry currently behaves exactly like an ordinary immediate note trigger —
`note_on` either updates the running macro's note/volume/transpose in place (if the same macro is
already running) or does a full retrigger — with **no gliding/sliding period at all**. This is not
a subtle miscalculation; the feature is unwired.

### Corroborating report

`turrican intro`, pattern `0x6b` (107), step 9:

```
9: Note { note: 23, macro_number: 1, volume: 0, voice: 0, timing: Portamento(6) }
```

The editor shows this as raw byte `$D7` with note name "F-2" — consistent with our decode
(`$D7 - $C0 = 23`, i.e. `NoteTiming::Portamento` fires correctly off `aa > $BF`; `dd = 6` is the
dropped rate). The user confirmed by ear: this step's slide is "not rendered correctly" in our
crate's output. That matches the code, not just a vague impression.

**Re-reported independently, 2026-08-04**: same pattern, same step, described as "macro 0x6b does
not play the note F-2/`$D7` at step 9 with portamento (speed 6)" — a second, independent hit on the
exact same root cause. Still no code change; still blocked on the design question below, which §16
(same session) makes harder, not easier, to answer.

### Open design question for the fix

`$FC <Port>` (a separate trackstep-line command, decoded at `tfmx/src/sequencer.rs:500` /
dispatched at `tfmx/src/player.rs:445`) is the crate's only existing portamento mechanism
(`MacroInterpreter::start_portamento`, `tfmx/src/macro_interp.rs:108-140`): every `speed` jiffies,
multiply the current period by `(256+rate)/256`, indefinitely, with **no target period and no stop
condition**. But a portamento-to-note record's whole point is to glide *toward a specific note*
and (presumably) stop on arrival — a different shape than the open-ended multiply `$FC`/`$0B`
already implement. Before wiring `dispatch_pattern_entry` to call `start_portamento`, decide:

- Does `dd` map to `rate` directly (with some derived/implicit `speed`), or something else?
  Neither [S1] excerpt already in `docs/opcodes.md` gives a worked numeric example for this record
  shape specifically — only for `$0B`/`$FC`.
  What is `macro_number` (`bb`, decoded as `1` in the example above) doing here — docs/opcodes.md
  line 105-107's general note-longword layout says `bb` is always "macro to play it with," even
  for portamento notes, but that's worth independent confirmation: does the target macro actually
  differ from whatever's already running on that voice, and if so, should a portamento entry
  really call `note_on` (which retriggers or updates in place) *and* start a glide, or does
  landing on a different macro number for a portamento note mean something else entirely?
- Does the existing `Portamento` struct need a target-period field and an arrival check, or does
  the crate need a second, distinct effect type for "glide to note" vs "open-ended multiply"?

### Where to look next

- `tfmx/src/player.rs:382-420` (`dispatch_pattern_entry`) — the fix site.
- `tfmx/src/macro_interp.rs:108-140` (`Portamento` struct) — likely needs a target-arrival mode.
- `docs/opcodes.md` §2 (lines 93-115) and §4's portamento diagram (if any) for anything already
  recorded about the note-record portamento's rate encoding.
- A regression test at the `dispatch_pattern_entry`/`Player::render` seam, once the semantics are
  settled — this is exactly the kind of control-flow bug the existing `player.rs` test style
  (`docs/architecture.md` §3) already covers well for other pattern commands.

---

## 2. RESOLVED (tooling convention, not an engine bug): pattern 0x54 / macros 0x30-0x31 "sound off, faster than the editor preview"

Investigated in the previous session (see conversation, not yet written to a doc before this one).
`turrican intro`, pattern `0x54` (84), voice 2, alternates macro `0x30`/`0x31` — a wavetable-frame
chorus/doubling effect (`$11 AddBegin` nudges the sample pointer by `+$40`/`-$40` between two
otherwise-identical macros). Structural checks against the trace (`tfmx-cli trace --voice 2`) all
matched the documented formulas exactly: combined transpose (track `-24` + macro's own `-6` = note
3), `note_period()`'s output, `$0D AddVolume`'s `coarse×3 + aa`, the envelope's decay curve, and
`$02 SetBegin`'s byte-offset decode. No arithmetic bug found there.

**The pitch discrepancy the user first reported turned out to be a context mismatch, not a bug**:
the editor's macro-preview auditions the macro in isolation (no trackstep transpose), while our
in-song render correctly applies track 0's `-24` transpose from trackstep line 76 (confirmed
`$E8` in the editor) plus the macro's own `-6`. Once isolated the same way — see the new
`render-macro` tool below — **the pitch matched the editor's preview.**

**What's still open**: the isolated render still "sounds off, like faster than the macro preview."
Prime suspect: `render-macro`'s `--tempo` flag (jiffy rate) defaulted to `0` (50 Hz, the *fastest*
possible rate) for the first comparison — 4x faster than this song's actual tempo 3 (12.5 Hz,
confirmed correct in `docs/trackstep-timing-bug.md` §3). All of the macro's own effect timing
(`$0F Envelope` `every=1`, `$04 Wait*`, the `$11 AddBegin` cadence) is jiffy-relative, so a 4x
tick-rate difference would make the whole macro's rhythm/decay noticeably faster. Re-rendered at
`--tempo 3` (matching the song) — but the user's ear says this direction was backwards.

**Confirmed by ear (2026-08-01)**: `--tempo 3` (the song's own tempo) sounds *too slow*;
`--tempo 0` (50 Hz, `render-macro`'s default) sounds right, matching the editor's preview. So the
editor's macro-audition feature does not preview at the song's tempo at all — it always plays the
macro at the fastest jiffy rate, independent of whatever song/tempo the macro happens to belong
to. Not an engine bug: `render-macro --tempo 0` (the default, no flag needed) is already the
correct comparison point against the editor's macro preview; only an *in-song* render should ever
use the song's own tempo.

---

## 3. OPEN, unconfirmed: pattern 0x6b / macros 0x0a (10) and 0x27 (39) "timbre/sound not correct"

Same pattern as §1's example (`turrican intro` pattern `0x6b`/107). Full disasm:

```
pattern 107:
   0: Note { note: 21, macro_number: 10, volume: 0, voice: 0, timing: Wait(3) }
   1: Command(Envelope { amount: 1, speed: 0, voice: 0, target: 64 })
   2: Command(Wait { jiffies: 27 })
   3: Command(Envelope { amount: 1, speed: 0, voice: 0, target: 48 })
   4: Note { note: 24, macro_number: 39, volume: 0, voice: 1, timing: Detune(0) }
   5: Note { note: 28, macro_number: 39, volume: 0, voice: 3, timing: Wait(3) }
   6: Command(Envelope { amount: 1, speed: 0, voice: 1, target: 24 })
   7: Command(Envelope { amount: 1, speed: 0, voice: 3, target: 24 })
   8: Command(Wait { jiffies: 26 })
   9: Note { note: 23, macro_number: 1, volume: 0, voice: 0, timing: Portamento(6) }   <- see §1
  10: Command(Wait { jiffies: 32 })
  11: Note { note: 19, macro_number: 10, volume: 12, voice: 0, timing: Wait(15) }
  12: Command(Envelope { amount: 1, speed: 0, voice: 1, target: 2 })
  13: Command(Envelope { amount: 1, speed: 0, voice: 3, target: 2 })
  14: Command(Stop)

macro 10:                                  macro 39:
   0: $00 <DMAoff+Reset*>                     0: $00 <DMAoff+Reset*>
   1: $02 <SetBegin> bb=$76 cc=$04             1: $02 <SetBegin> bb=$90 cc=$04
   2: $03 <SetLen> bb=$08 cc=$00               2: $03 <SetLen> bb=$11 cc=$80
   3: $0D <AddVolume> cc=$08                   3: $0D <AddVolume> cc=$10
   4: $08 <AddNote*> aa=$06                    4: $08 <AddNote*> aa=$06
   5: $01 <DMAon>                              5: $01 <DMAon>
   6: $04 <Wait*>                              6: $04 <Wait*>
   7: $18 <Sampleloop> bb=$0F                  7: $18 <Sampleloop> bb=$06 cc=$A6
   8: $1A <Wait on DMA*>                       8: $14 <Wait key up*> cc=$0C
   9: $04 <Wait*>                              9: $0C <Vibrato> aa=$05 cc=$FE
  10: $1A <Wait on DMA*>                      10: $14 <Wait key up*>
  11: $04 <Wait*>                             11: $0C <Vibrato> aa=$04 cc=$FF
  12: $11 <AddBegin> bb=$FF cc=$00 (-256)      12: $0F <Envelope> aa=$04 bb=$01 cc=$00
  13: $05 <Loop> aa=$0E bb=$00 cc=$0B          13: $04 <Wait*>
  14: $04 <Wait*>                              14: $07 <STOP*>
  15: $11 <AddBegin> bb=$01 cc=$00 (+256)
  16: $05 <Loop> aa=$0E bb=$00 cc=$0E
  17: $05 <Loop> aa=$00 bb=$00 cc=$0B
  18: $04 <Wait*>
  19: $04 <Wait*>
  20: $07 <STOP*>
```

Macro 10 alternates the sample-read pointer by `-256` (14x, steps 11-13 looped) then `+256` (14x,
steps 14-16 looped) — expected to *net to zero*, an oscillating scrub through a wavetable, gated by
`$1A <Wait on DMA>` on real sample-loop completions rather than fixed jiffy counts.

**Anomaly confirmed 2026-07-31, with `JIFFY` lines intact this time**
(`tfmx-cli trace --voice 0 --song 0 --seconds 15 --gate any`): `start` does not oscillate — it
climbs monotonically, one `VOICE` row per real `JIFFY` row, by a near-constant **+74752**
(= 292×256) per row, well past where the ±256 alternation should have turned around. This is a
real per-real-jiffy delta, not a trace-filtering artifact.

**Root cause candidate, not yet confirmed**: `tfmx-cli disasm --macro 10` shows the two `$04
<Wait*>` steps inside the scrub loop (steps 11, 14) genuinely have `aa=$00 bb=$00 cc=$00` in the
raw module data — real data, not a decode bug, and per `docs/opcodes.md:152` ("Waits `aaaa`
jiffies", no +1) a zero-wait correctly does not suspend in the current code
(`MacroInterpreter::execute`, `$04` arm). That's expected: `$05 <Loop>`'s two finite passes (`aa
= $0E` = 14 each, steps 13 and 16) are meant to run as an instantaneous burst within one jiffy,
with only the two `$1A <Wait on DMA>` calls before them (steps 8, 10) providing real per-jiffy
pacing — that much matches spec.

The suspect is step 17: `$05 <Loop> aa=$00 bb=$00 cc=$0B` — an **unconditional** jump back to
step 11, because `MacroInterpreter::execute`'s `$05`/`$10` arm
(`tfmx/src/macro_interp.rs:618-635`) treats `aa == 0` as "infinite repeat," by silent analogy
with the pattern-level `$F1 <Loop>` (`docs/opcodes.md:120`, which *does* explicitly document
`aa=0` → "repeats indefinitely"). **`docs/opcodes.md:153`'s row for the macro-level `$05
<Loop>` states no such thing for `aa=0`** — the "infinite" reading was borrowed from `$F1`, never
independently sourced. No existing test discriminates the two readings:
`loop_key_up_breaks_out_early_once_signaled` uses `aa=0` on `$10` but also signals key-up before
the first pass, so it passes identically whether `aa=0` means "infinite" or "no loop, fall
through" — `$05`'s `aa=0` case has no test at all.

If `aa=0` means "don't loop, fall through" instead of "infinite," macro 10 becomes a well-formed,
finite composition: two 14-rep bursts (`-256×14` then `+256×14`, net zero — exactly the
"oscillating scrub" originally expected) then a genuine `$07 <STOP*>` at step 20, rather than an
infinite loop that only terminates in our interpreter because of `MAX_MACRO_OPS_PER_JIFFY = 1024`
(`tfmx/src/macro_interp.rs:241`), a runaway-loop guard invented for this crate with no basis in
the spec — real hardware has no such cap, so an actually-infinite `$05 Loop` here would hang the
voice's macro processing forever, which is hard to square with the game shipping working music.

**Update (2026-07-31): editor-confirmed `$05 Loop aa=0` loops forever — that redirected the
diagnosis, and found a different, now-fixed bug.** Since the loop really is infinite, something
inside it must consume real time each pass, or the voice would hang forever on real hardware.
`$04 <Wait>*` is asterisked (`docs/opcodes.md` §3 intro: asterisked opcodes "can suspend the
voice's macro program for one or more jiffies"), and its siblings `$08`/`$09` unconditionally
suspend for at least one jiffy regardless of operand value. But `$04`'s own handling special-cased
`word23 == 0` as "don't suspend at all" — inconsistent with its own siblings in the same `match`,
and exactly reproducing the observed symptom (the whole loop spinning at full speed every tick,
bounded only by the invented `MAX_MACRO_OPS_PER_JIFFY`, instead of pacing one `$11 AddBegin` per
real jiffy).

**Fixed, TDD'd**: `tfmx/src/macro_interp.rs`'s `$04` arm now always suspends for
`word23.saturating_sub(1)` further jiffies (i.e. always yields at least the current jiffy, same as
`$00`'s own `aa == 0` case), never skips suspension. New test
`wait_with_zero_jiffies_still_suspends_one_jiffy` pins this red-to-green; all 139 `tfmx` unit tests
and `mutation_robustness` still pass. Re-traced `turrican intro` macro 10 post-fix: `start` now
genuinely oscillates by exactly ±256 per jiffy (`30212 → 29956 → … → 26372`, turns around, `→ …  →
30212`, turns around again) instead of climbing monotonically — the "oscillating scrub through a
wavetable" originally expected. **Not yet heard**: this changes real rendered audio (the golden
hash for `apidya (level 1)` already differs), so per the standing rule this isn't done until the
user's ears confirm it, and the golden hashes stay unregenerated until then.

### Rendered for A/B (regenerate with the commands below; scratch files don't persist)

```
tfmx-cli render-macro "testdata/mdat.turrican intro" "testdata/smpl.turrican intro" \
  -o macro10_note21.wav --macro 10 --note 21 --volume 64 --voice 0 --tempo 3 --seconds 4
tfmx-cli render-macro "testdata/mdat.turrican intro" "testdata/smpl.turrican intro" \
  -o macro39_note24.wav --macro 39 --note 24 --volume 64 --voice 1 --tempo 3 --seconds 4
tfmx-cli render-macro "testdata/mdat.turrican intro" "testdata/smpl.turrican intro" \
  -o macro39_note28.wav --macro 39 --note 28 --volume 64 --voice 3 --tempo 3 --seconds 4
```

Not yet A/B'd against the editor's own preview of these two macros.

---

## Tooling added this session: `tfmx-cli render-macro`

New subcommand, `tfmx-cli/src/main.rs`. Triggers a single macro directly — no `Sequencer`, no
trackstep/pattern layer, no track transpose — by driving `MacroInterpreter` + `Paula` directly,
mirroring `Player::render_inner`'s tick-then-mix loop (`tfmx/src/player.rs:183-227`) at
single-voice scale. This is the same seam `MacroInterpreter`'s own unit tests already drive
standalone (e.g. `take_turn_resumes_once_loop_completions_reach_target`,
`tfmx/src/macro_interp.rs:1559`).

```
tfmx-cli render-macro <mdat> <smpl> -o out.wav --macro N --note N \
  [--volume 64] [--voice 0-3] [--tempo N] [--seconds N] [--rate 44100] [--separation 100]
```

**Gotcha, learned the hard way (§2 above): `--tempo` defaults to `0` (50 Hz, the fastest possible
jiffy rate)**, not any particular song's tempo. All of a macro's own effect timing is
jiffy-relative, so comparing against an editor preview (or any in-song render) at the wrong tempo
will make everything sound uniformly faster/slower without any real bug being involved. Pass
`--tempo` matching whatever you're comparing against, and find out what rate the editor's own
preview uses before trusting a "sounds faster" verdict.

Regression test: `render_macro_writes_a_wav_of_the_requested_length`, next to the existing
`render`-command test in `tfmx-cli/src/main.rs`'s test module.

---

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

## 6. RESOLVED (tooling gotcha, not an engine bug), found 2026-08-01: `render-macro --note <raw editor byte>` renders silent

While isolating pattern `0x54` (84) with the newly-added `render-pattern`/`render-macro` commands:
the user tried `render-macro --macro 48 --note 161` (`0xA1`, the editor's raw byte for this note,
named "D#3" there) and heard nothing.

**Root cause, confirmed by arithmetic, not just observation**: `161` is the *raw pattern-longword
note byte*, not the crate's internal pitch value. Real pattern decoding
(`decode_pattern_entry`/`NoteTiming`, `tfmx/src/sequencer.rs`) treats a note byte in `$80`-`$BF` as
`NoteTiming::Wait`, with the actual pitch masked to the low 7 bits: `0xA1 & 0x7F = 0x21 = 33`.
Confirmed against the real module: `tfmx-cli disasm --pattern 84` shows pattern `0x54`'s entry
using macro 48 as `Note { note: 33, macro_number: 48, volume: 12, voice: 2, timing: Wait(31) }` —
**33**, not 161. `render-macro` bypasses pattern decoding entirely by design (that's its whole
point, per its own doc comment) and calls `MacroInterpreter::trigger()` directly with whatever
`--note` says, unmasked. Passing the raw `161` computes `note_period(161, 0)`
(`tfmx/src/macro_interp.rs:23-28`): `161 - MIDDLE_C_NOTE(30) = 131` semitones up, `freq ≈
8363 × 2^(131/12) ≈ 15.9 MHz`, `period = PAULA_CLOCK_HZ / freq` rounds to `0`. `Paula::render`
(`tfmx/src/paula.rs:57`) explicitly silences any voice whose period is `0`. That fully explains
the reported silence — no engine bug, no dispatch bug, just the wrong argument value.

**Not a real playback bug**: in an actual song render, `dispatch_pattern_entry` always receives
the already-masked `note` field from `PatternEntry::Note` (as `disasm` shows above), never the raw
byte — so this failure mode is specific to `render-macro`'s direct-trigger CLI path, not to
anything a real module ever hits.

**Tooling improvement done (2026-08-01)**: `render-macro --note` now masks any raw byte to its low
6 bits (same as real pattern decoding) before triggering, so pasting the editor's raw packed-record
byte (e.g. `$A1`) no longer needs manual arithmetic and can no longer silently land on a
period-rounds-to-0 value. It also accepts a note name directly (`C-3`, `F#0`, ...), the editor's own
table spelling — see `parse_note`/`NOTE_NAMES` in `tfmx-cli/src/main.rs`. `render-pattern` has no
`--note` flag (its notes come from the pattern data itself), so this only applied to `render-macro`.

---

## For the next session

0. **Done**: `render-pattern --transpose` now accepts a raw hex byte (`0xE8`/`$E8`) in addition to
   plain signed decimal, via a `parse_transpose` value-parser mirroring `parse_note`
   (`tfmx-cli/src/main.rs`), TDD'd (3 new unit tests, full `tfmx-cli` suite green apart from the
   pre-existing, unrelated golden-hash mismatch tracked in `docs/trackstep-timing-bug.md`).
1. **Done — §7**: the disconnect is explained, not a no-op. A differential render (macro-only,
   pattern-only, full-song, pre/post §3/§4) proves the fixes change the PCM enormously (up to 93.5%
   of samples differ, RMS(diff) > RMS(signal) at every scope) — "sounds the same" was about
   persistent *character*, not an unchanged waveform. §1 and §5 remain the leading suspects for
   that character since both are still unfixed in every render compared. Next: fix one of them and
   re-run the same three-scope differential to see whether the character (not just the waveform)
   finally shifts.
2. **§5's `$18 Sampleloop` byte/word unit bug is now FIXED and TDD'd** (2026-08-01) — the design
   question ("is the out-of-bounds `loop_len` a decode bug?") is answered yes: the delta needed
   halving before subtracting from the word-valued `loop_len`. Dropped corpus-wide
   `sample-region-out-of-bounds` findings from 26 to 9 voice-instances. **Not yet heard.** Still
   open: 9 remaining out-of-bounds voice-instances (including a second, still-unidentified region
   on `turrican intro` voice 0 itself) are a different bug (or bugs) — not yet traced to a specific
   macro/opcode, and not addressed by this fix.
3. **§1 (portamento) is ready to fix** once the design question (rate encoding, target-arrival
   semantics, what `macro_number` means on a portamento note) is settled — ask the editor, or look
   for a worked example in whatever source informed `docs/opcodes.md` originally.
4. **§2 is resolved** — the editor's macro-preview always plays at the fastest jiffy rate
   regardless of song tempo; use `render-macro`'s `--tempo 0` default when A/B-ing against it,
   never the song's own tempo.
5. The `frozen-voice`/`clipping` lint findings noted at the end of §5 haven't been investigated —
   worth a look if §1/§5 don't fully explain the remaining "off" quality.
6. None of this blocks or is blocked by the trackstep-timing golden-hash regeneration
   (`docs/trackstep-timing-bug.md`) — they're independent bugs found via the same editor
   cross-check session.
7. §6 is resolved (tooling gotcha, not a bug), and its suggested guard is now implemented:
   `render-macro --note` masks any raw byte to its low 6 bits and also accepts a note name
   directly, so the editor's raw packed-record byte or note name can be pasted straight in without
   the `disasm`/masking detour this item used to require.
8. **§9 (2026-08-01): two more real bugs fixed and TDD'd** (`$11 AddBegin` fighting the attack→loop
   handoff; `$05 Loop`'s shared repeat-counter poisoned by the unconditional `aa=0` form) —
   `turrican intro` voice 0's `sample-region-out-of-bounds` finding is fully gone. **But the user's
   re-listen still finds voice 0 too low-pitched and still hears the playhead wander into the wrong
   sample areas.** §9's three leads: (a) resolved by §11 — `$11`'s wobble is NOT the cause, disabling
   it makes voice 0 sound worse (do not touch); (b)/(c) resolved by §12 — see item 9 below. 9
   `sample-region-out-of-bounds` findings remain across other corpus modules, unexplored — do not
   assume they share today's root causes without checking.
9. **§12 (2026-08-01, session 10) — CHOSEN NEXT STEP, START HERE**: theory 2 (editor ground truth for
   macro 28) is done — the editor's own disassembly of macro 28 matches `tfmx-cli disasm --macro 28`
   byte-for-byte (no decode bug), and a manual sweep test in the editor independently validated the
   `$18 Sampleloop` loop-length formula by ear, matching its predicted `delta=0x800` breakpoint
   exactly. **`$18`'s values are now ruled out** as the cause of "too low pitch"/"wanders" — this was
   the last untested link in the pitch chain (`note_period()` confirmed §6, `$18` bounds fixed §5,
   `$18` values confirmed §12, `$11` wobble ruled out §11).
   **The live lead going into a fresh session**: `docs/opcodes.md` describes `$02 SetBegin` as adding
   to "the sample's **base address**" (a fixed noun) vs. `$11 AddBegin` adding to "the sample
   **pointer**" (explicitly the live/oscillating position) — two different phrases for what might be
   two different things. The code (`tfmx/src/macro_interp.rs:606-613`) currently implements both as
   accumulation onto the same `self.sample_start` field, i.e. treats "base address" and "pointer" as
   identical. Macro 28 issues **two** `$02 SetBegin` calls per trigger (`+0x1C14` then `+0x7804`) —
   the one case in this whole thread where cumulative-vs-absolute actually changes *which bytes get
   read*, not just bounds: cumulative (current code) → `sample_start=37912`; a fixed-base reading
   could land somewhere else entirely (e.g. `30724`-relative). Before writing any code:
   - This rests on one sentence per opcode with no corroborating passage — re-read
     `docs/opcodes.md` §3's `$02`/`$11` entries and `docs/format.md` §8 for anything missed.
   - Check whether "the sample's base address" could mean something *other* than 0 (e.g. a
     per-instrument constant from elsewhere in the format) before assuming `delta` alone is the
     answer — `docs/format.md`'s only worked `$02` example has just one call, so it can't settle this.
   - `sampleloop_keeps_turrican_intro_macro_28_in_bounds` (`tfmx/src/macro_interp.rs:1607`) already
     pins the current cumulative reading with the real macro-28 numbers — it will need to change if
     this theory is acted on, and its replacement value needs deriving from whatever "base" turns out
     to mean, not guessed.
   - Sweep corpus-wide impact (how many macros issue 2+ `$02` calls per trigger?) before fixing, the
     same way §5/§7 did, since this could be a much bigger change than any fix so far in this thread.
   - TDD required (per project convention): write the failing test first from whatever the settled
     semantics turn out to be, then fix, then re-render for the user's ears — per this thread's own
     repeated lesson, do not assume a structurally-correct fix is audibly meaningful until confirmed.

## Tooling added this session (2026-08-01): `tfmx-cli render-pattern`

New subcommand alongside `render-macro`, `tfmx-cli/src/main.rs`. Drives one `PatternRunner` + the
4-voice `MacroInterpreter` array + `Paula` directly — no `Sequencer`/trackstep, so no live
per-jiffy transpose refresh. `--transpose` (default 0) and `--tempo` (default 0, same "50 Hz
fastest possible" gotcha as `render-macro` — see above) stand in for what the trackstep row would
otherwise supply. Confirmed: a pattern's `Note` entries carry `note`/`macro_number`/`volume`/
`voice` entirely within the pattern data itself (`dispatch_pattern_entry`, `tfmx/src/player.rs`)
— transpose is the *only* thing a real trackstep row contributes, so this is a faithful isolation
of one pattern's own behavior, useful for cases like §5 (silence traced to one pattern/macro pair)
without needing a full song render.

```
tfmx-cli render-pattern <mdat> <smpl> -o out.wav --pattern N \
  [--transpose 0] [--tempo N] [--seconds 10] [--rate 44100] [--separation 100]
```

Known simplification: `$FB <PPat>`'s `track` operand is dropped (treated as "replace the running
pattern") since a standalone pattern has no second track to jump to — covers self-loop/chain
patterns, not a true multi-track jump. Smoke-tested against `turrican intro` pattern 21; not yet
used for a real A/B comparison.

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

## 10. RESOLVED (tooling gotcha, not an engine bug), found 2026-08-01 (session 8): isolating macro 28 via `render-macro` is invalid — it goes silent after ~60ms

Attempted §9's theory 3 (isolate macro 28 alone via `render-macro` + `measure-pitch`, compare against
the editor's macro-audition). `render-macro --macro 28 --note 0x21 --volume 64` renders audio only in
samples 1764-4410 (a ~60ms burst) of a 1-3s file, then **hard silence for the rest of the render** —
so the resulting `measure-pitch` reading (8820 Hz) is worthless, almost certainly measured off that
one short burst plus its attack transient, not the steady-state loop tone.

Traced with a temporary per-jiffy `eprintln!` (added, inspected, reverted — not in the tree) printing
`self.volume`/`dma_on`/loop registers: `dma_on` stays `true` and the loop registers stay
well-formed and in-bounds the whole time (confirming §5/§9's fixes are not implicated) — but
`self.volume` drops to `0` at step 13 and **never recovers**, exactly when macro 28's step 11
(`$0E <SetVolume> aa=$00 bb=$00 cc=$38`) executes. `docs/opcodes.md` §2's `$0E` row documents the
operand layout as `aa xx xx` — the code reads `b1` (`aa`) as the absolute volume
(`self.volume = b1.min(64)`), and the real macro's `aa` byte here genuinely is `0`, so this is not a
decode bug: the macro literally zeroes its own volume register at that step.

**Why this doesn't reach the ear in the real song**: pattern `0x52` retriggers macro 28 every 1-3
jiffies (§9's own Recipe A finding), and `MacroInterpreter::note_on` (`tfmx/src/macro_interp.rs:389-397`)
takes the "same macro number, still running" branch on every one of those retriggers — which
unconditionally overwrites `self.volume` from the pattern's own `cv` volume nibble
(`self.volume = volume.min(15) * 3`) without resetting `self.step`. So the macro's program counter
free-runs at its own pace (steps 11/13's `$0E` executes exactly once, ever, since the outer loop at
steps 17-23 never revisits it), and whatever it sets is overwritten by the next retrigger within at
most 3 jiffies (60ms) regardless. In-song, this SetVolume(0) is a real but likely single, sub-60ms,
probably-inaudible dip near the note's own attack — not a plausible source of the persistent
"too low pitched, wanders" complaint. `render-macro`, which triggers once and never retriggers, has
no such recovery, so it renders as if the note died — a tooling artifact, not a playback bug. Same
shape as §2 (tempo mismatch) and §6 (raw vs. masked note byte): `render-macro` isolates a macro from
its pattern context, and for a macro that depends on that context's retriggering to stay audible past
its own internal volume-zeroing step, isolation itself is the wrong tool.

**Consequence for §9's next steps**: theory 3 (isolate via `render-macro`) is not viable for macro 28
as originally proposed — use `render-pattern --pattern 82` instead (preserves the real retrigger
cadence) if a `measure-pitch` reading on this macro/voice is still wanted. Theories 1 (no-op the `$11`
wobble and compare) and 2 (get the editor's own loop-point ground truth for macro 28) are unaffected
by this finding and remain the more promising next steps — neither depends on single-shot macro
isolation.

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

## 15. New, independently-confirmed bug: voice 2's macro-internal pulse rate is ~3.5x too slow

**Not the same complaint as pitch — a separate defect, found while re-checking §2.** The user
reports pattern `0x54`/macros `0x30`-`0x31` (voice 2, the wavetable-frame chorus/doubling effect)
still "sounds like a slower version" of the real editor's playback, *pacing/density*, not pitch.
Corroborating experiment: in the editor, manually increasing macro `0x30`'s `$04 <Wait>*` operand
from `6` to `0x1A` (26) made the *editor's* own song-context playback match this crate's (wrong)
speed — strong evidence this is a real jiffy-counting defect, not a subjective impression.

**§2 never actually tested this** — it structurally checked transpose/`note_period()`/`$0D`/
envelope-curve/`$02` *formulas*, and separately explained away the *editor's macro-preview*
tempo mismatch (50Hz vs song tempo) as a tooling artifact. Neither check exercised the macro
interpreter's own internal `$04 <Wait>*`/`$0F <Envelope>` *timing* against real hardware — this
session is the first time that's been done.

**Objective measurement, this session:**
- `tfmx-cli render-pattern --pattern 84 --transpose=-24 --tempo 3` (isolates the pattern from
  trackstep/gating) vs. a user-provided phone recording of the real editor playing pattern `0x54`
  in-song, voice 2 soloed (`testdata/` not committed — session-scratchpad WAVs only).
- `measure-pitch` on both: 155.28 Hz (crate) vs. 154.20 Hz (editor) — **pitch matches**, ruling out
  a timbre/note mismatch as a confound for the next two measurements.
- `onset-diff`: crate 39 onsets/21.0s (1.9/s) vs. editor 128 onsets/20.7s (6.2/s) — a **~3.3x**
  gap, essentially zero inter-onset correlation (0.018).
- Autocorrelation of the editor recording's 20ms RMS envelope (steady-state region, skipping the
  attack) finds a clean fundamental period at **160ms**, with harmonics at 320/480/640ms — not
  noise (ruled out background-recording-noise as an alternative explanation for the onset-diff gap
  by inspecting the raw envelope directly: the periodicity is visually obvious, not just an
  aggregate statistic).
- A throwaway test (`macro_interp.rs`, written, run, reverted — `git status` clean) ran macro 48's
  *exact* program (from `disasm`) standalone and printed DMA/volume state per jiffy: confirms this
  crate's own internal loop (steps 9-15: `DMAoff*`→`AddVolume`→`AddNote*`(1j)→`DMAon`→`Envelope`→
  `Wait*`(6j)→`AddBegin`, looping unconditionally to step 9) takes exactly **7 jiffies** per cycle
  — matching the `$04 <Wait>*` operand literally and matching every documented per-opcode
  suspend rule checked individually (`$04` gap independently confirmed as exactly 6 jiffies via a
  second, isolated throwaway test). **The code does exactly what the opcodes and the docs say —
  and that's 3.5x too slow.** 7 jiffies at the song's 12.5 Hz (tempo 3) = 560ms; the editor
  measures 160ms = **exactly 2 jiffies at 12.5Hz**.

**Working theory, not yet implemented or confirmed further: TFMX may have two independent clock
domains, not one.** `docs/playback-model.md` §1 (the signal-chain intro, uncited to `[S1]`)
currently states trackstep, pattern, *and* macro program all advance on the *same* tempo-scaled
jiffy — this crate has always implemented that as one unified clock
(`tfmx/src/sequencer.rs`'s `tick_fraction`, called once per jiffy, driving `Player::run_jiffy`
which ticks trackstep/pattern/every macro together). But **every prior timing validation in this
whole project only ever tested the trackstep/pattern side** of that claim (the `docs/trackstep-
timing-bug.md` stopwatch experiment used a plain `Wait(31); End` *pattern*, no macro effects
involved) — the "macro also runs on the scaled clock" half has never been checked against real
hardware until this session, and this session's numbers argue against it: if the macro interpreter
instead ticks at the **raw 50Hz hardware rate, independent of the song's trackstep tempo divisor**
(i.e. trackstep/pattern advance is tempo-scaled per `docs/trackstep-timing-bug.md`'s already-
confirmed formula, but `$04`/`$0F`/`$0B`/`$0C`/`$11`'s own jiffy counters are *not* — they always
count raw 50Hz ticks), the same 7-op-jiffy internal loop would take 7/50s = **140ms**, within
~14% of the measured 160ms — far closer than the current model's 560ms (3.5x off). Not proven:
140ms vs. 160ms isn't an exact match, and no `[S1]` passage has been found yet stating this
explicitly (or denying it) — this needs a primary-source re-check and, if adopted, is a **wide-
blast-radius architecture change** (decoupling the macro tick clock from `tick_fraction`
throughout `player.rs`/`sequencer.rs`), unlike every per-opcode fix earlier in this thread.
**Not yet implemented — chosen next step for a fresh session, pending the user's direction.**

**Paused here (2026-08-01, user's call) — reproduction steps for whoever picks this up:**

Reference recording (external, not in the repo): `/Users/mrolappe/Nextcloud/drehscheibe/
turripat54.m4a` — the real TFMX editor playing `turrican intro` pattern `0x54` in song context,
voice 2 soloed/others muted, ~20.7s, 44.1kHz mono AAC. Convert with
`ffmpeg -y -i turripat54.m4a -ar 44100 -ac 1 editor-pattern54-voice2.wav`.

This crate's comparison render (session scratchpad, not committed — regenerate):
```
tfmx-cli render-pattern "testdata/mdat.turrican intro" "testdata/smpl.turrican intro" \
  --pattern 84 --transpose=-24 --tempo 3 --seconds 21 -o crate-pattern84-voice2.wav
```

Measurements to reproduce:
- `tfmx-cli measure-pitch <wav>` on both — expect ~155 Hz both sides (pitch already confirmed
  matching, not the thing to re-check).
- `tfmx-cli onset-diff crate-pattern84-voice2.wav editor-pattern54-voice2.wav` — expect ~1.9
  onsets/s (crate) vs. ~6.2 onsets/s (editor), correlation near 0.
- A Python/numpy script computing a 20ms RMS envelope and its autocorrelation on the editor WAV
  (steady-state region, e.g. windows 50-400, skip the attack) finds the clean 160ms period; no
  ready-made CLI tool for this yet, ad hoc this session.

Next concrete step: re-check `[S1]` (`docs/format.md`'s Sources table, tag S1) specifically for
any statement about whether macro-opcode timers (`$04`/`$0F`/`$0B`/`$0C`/`$11`) run on the song's
tempo-scaled jiffy or the raw 50Hz hardware rate — this project's own docs currently assert the
former (`docs/playback-model.md` §1) without a citation, and no prior session tested it. If `[S1]`
is silent (as most of §3's timing section already is on edge cases), the only way to gain more
confidence before attempting the refactor is another *controlled* editor experiment analogous to
the `docs/trackstep-timing-bug.md` stopwatch test — e.g. author a minimal macro with a single
`$04 <Wait>* aaaa=N` between two audible markers, at two different song tempos, and check whether
the real-time gap changes with tempo (unified clock) or stays fixed (raw-50Hz clock).

---

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

## 17. FIXED, ear-confirmed: r-type macro 13's sweep was stuck on a static loop fragment — two stacked bugs

New report (2026-08-04, separate session): r-type macro 13 sounded "like a small sample fragment is
looped indefinitely," but the real macro (auditioned in the TFMX editor) sweeps through different
sample areas. `tfmx-cli disasm --macro 13` showed the mechanism: `$11 <AddBegin>` (periodic form,
`half_period=$FF=255`, `step=$FEE0=-288`) is the first instruction inside a `$05 <Loop>` body
repeated 254 times — the loop count almost exactly matches the vibrato's half-period, clear intent
for one continuous ramp across the whole loop, paired with `$12 <AddLen>` growing the region by +4
words every pass.

Two stacked bugs, both in `tfmx/src/macro_interp.rs`:

1. **Phase reset on re-arm** (the `$11` opcode handler): every re-execution of `$11` inside the loop
   rebuilt `PointerVibrato` with `t: 0`, so `PointerVibrato::delta()` always read 0 the instant it
   was called — the pointer never advanced past the very start of its ramp. Fixed: re-arming an
   already-running vibrato now updates `half_period`/`step` in place and preserves `t`.
2. **Deferred writes never reaching the loop target** (`MacroInterpreter::tick`): DMA turns on
   before the loop starts, so `Paula::set_sample_region`'s writes defer to the next natural wrap
   (`docs/playback-model.md` §2.3's double-buffering model, landed the session before this one,
   commit `fe48dc3`) — and `Voice::next_sample`'s wrap always reloads `loop_start`/`loop_len`, not
   whatever was most recently requested. Nothing kept `loop_start`/`loop_len` in sync with `$11`/
   `$12`'s changes absent an explicit `$18 <Sampleloop>`, so even with bug 1 fixed the ramp was still
   silently discarded at every wrap. Fixed: `loop_start`/`loop_len` now continuously mirror the live
   attack pointer (`sample_start`/`sample_len`) whenever `$18` hasn't run yet (`loop_active == false`).

TDD'd with `add_begin_periodic_form_keeps_ramping_when_re_armed_inside_a_loop`
(`tfmx/src/macro_interp.rs`), reproducing macro 13's exact shape — fails before either fix, passes
with both. Full workspace suite green except golden hashes, which moved for 4 modules that exercise
this general pattern (not just r-type): `r-type`, `turrican intro`, `turrican 2 title (st)`,
`x-out (title)`. **User ear-confirmed** on `render-macro --macro 13` before/after
(`testdata/synth/rtype-mac13-{before,after}.wav`) and the full song 0 in context
(`rtype-song0-{before,after}.wav`): macro 13 now sweeps through sample content as expected.

### New, unconfirmed lead: an audible "blip" at the very start of the fixed `render-macro` output

The user also heard a brief blip/click right at the start of `rtype-mac13-after.wav` that is **not**
present when auditioning macro 13 directly in the real TFMX editor. Explicitly flagged as "not sure
whether it is related" and to be recorded for a future investigation, not chased this session.

What's been ruled out already: a sample-by-sample diff of `rtype-mac13-before.wav` vs.
`-after.wav` shows the two renders are **byte-identical for the first 2033 frames** (~46ms, well
past the note's onset at frame 1764) — the fix does not change anything about the attack itself,
so if the blip is new (introduced by this session's fix rather than pre-existing and simply masked
before by the stuck-loop symptom), it isn't at the very first sample-region write. Not yet checked:
whether the blip is a `render-macro` tooling artifact (this tool has a known-real limitation, §10
above, for macros that depend on pattern-level retriggering — macro 13 is triggered once by
`render-macro` with no pattern context, unlike in the real song) vs. a genuine new engine bug from
this session's `loop_start`/`loop_len` sync change, vs. something pre-existing that was simply
harder to notice under the old stuck-loop symptom. First things to try in a fresh session: (a)
`render-pattern` instead of `render-macro` for the same macro, since it preserves real trigger
cadence and would rule out the tooling explanation; (b) locate the blip's exact sample offset (the
diff above only checked the first 2033 frames — extend it) and cross-reference against `tfmx-cli
trace --macro 13` to see which opcode is executing at that jiffy.

## 18. NEW, root cause found, ear-confirmed, fix NOT chosen: pattern `0x52`(82)/macro `0x1c`(28) collapses some notes into the previous one instead of restarting

**User report (2026-08-04, separate session)**: `turrican intro` pattern 82's note lengths/durations
are not correct. This is the same pattern/macro §5-§12 already investigated (silence, then
out-of-bounds sample regions) — those fixes hold, `tfmx-cli lint` reports nothing for this
voice/macro any more — but a distinct bug in the same neighborhood survived.

### The pattern

`tfmx-cli disasm --pattern 82`: five notes, all voice 0, all macro 28, looping forever (`$F1 <Loop>
aa=0`):

```
0: Note { note: 33, macro: 28, volume: 10, timing: Wait(1) }
1: Note { note: 33, macro: 28, volume:  5, timing: Wait(1) }
2: Note { note: 33, macro: 28, volume: 10, timing: Wait(3) }
3: Note { note: 33, macro: 28, volume: 10, timing: Wait(1) }
4: Note { note: 33, macro: 28, volume: 10, timing: Wait(3) }
   -> loop to 0
```

Occupied jiffies before the next dispatch (`wait + 1`, `tfmx/src/sequencer.rs:655-661`): `2, 2, 4, 2,
4`, repeating. Traced against the real render (`tfmx-cli trace --track 3 --gate any`): the pattern
dispatches on exactly this cadence, confirmed against real frame deltas at tempo 3 (12.5 Hz, 80 ms/
jiffy) — `160, 160, 320, 160, 320` ms repeating. **Dispatch timing itself is correct**; the bug is in
what each dispatch actually does to the voice.

### Root cause: `note_on`'s `dma_on`-based "still sustaining" heuristic races macro 28's own attack latency

`MacroInterpreter::note_on` (`tfmx/src/macro_interp.rs:429-439`) skips restarting the macro program
— just updates note/volume in place — whenever the incoming Note names the instrument already
running on that voice and it's judged still "sustaining": `!self.dma_on` (pre-attack) or parked in
`$14 <Wait key up>` with no active envelope. This heuristic exists for three already-fixed, already
ear-confirmed cases (`docs/status.md`): macro 41's 1-jiffy-cadence retrigger (needs the swallow —
the retrigger is always faster than `$01 DMAon` could ever fire, so *never* restarting is the only way
sound survives at all), macro 38's 2-jiffy-cadence retrigger (needs the opposite — `$00 aa=1` skips
the mandatory pause, so `dma_on` is already true by the next retrigger and a genuine restart is
correct), and macro 8's `$14`-parked-but-already-decaying pluck (needs the envelope check to avoid
reading a fake sustain).

Macro 28 (`tfmx-cli disasm --macro 28`, full listing in §5) has the exact same `$00 aa=0` shape as
macro 41: `$00` (mandatory 1-jiffy pause) → `$02`/`$03`/`$0D` (immediate) → `$08 <AddNote*>`
(suspends) → `$01 <DMAon>`. That is **exactly 2 real jiffies** from `trigger()` to `dma_on` becoming
true. But unlike macro 41's uniform 1-jiffy cadence, pattern 82's cadence alternates 2 and 4 jiffies
— sometimes faster than, sometimes slower than, sometimes exactly equal to that 2-jiffy latency.
Dispatch happens before the current jiffy's macro tick (`docs/playback-model.md` §1's documented
signal-chain order, `run_jiffy`, `tfmx/src/player.rs:236-240`), so a retrigger landing exactly 2
jiffies after the previous one always finds `dma_on` still `false` — one tick before it would have
turned `true` — and takes the swallow branch instead of restarting.

**Confirmed by temporary instrumentation** (an `eprintln!` in `note_on`, added, traced, and reverted
— `git status` clean before and after): tracing pattern 82 live shows the actual per-note pattern is
`restart, swallow, restart, restart, swallow` each cycle — three real re-attacks and two silently
absorbed notes, not the five the pattern data encodes. Every `TraceEvent::Trigger` fires regardless
(`tfmx/src/player.rs:410-415` emits it unconditionally), so `tfmx-cli trace`'s own `TRIGGER` lines
cannot be used to tell restarts from swallows — that trace event is not evidence either way for this
class of bug.

### Isolated A/B, ear-confirmed

Built with `tfmx-cli render-pattern --pattern 82 --transpose 0 --tempo 3` (the pattern's real
transpose is 0 on track 3, confirmed from the trackstep trace; tempo 3 matches the song), plus a
trimmed `uade123` full-song reference (`uade123 -s 0 -t 22 -f uade-full.wav "mdat.turrican intro"`,
trimmed to the pattern's real 13.04s-21s window with `ffmpeg -ss 13.0 -t 8.0`) and this crate's own
full-song render (`--gate any`, both full mix and voice-0-solo) trimmed to the same window.

- **Quantitative, isolated render alone**: 5 notes/cycle over a 1.12s cycle should give ~27 onsets in
  6 seconds; `tfmx-cli onset-diff` on the isolated render against itself (for a raw count) reports
  only **16** — consistent with the 3-of-5 restart ratio found by instrumentation (allowing for the
  onset detector missing a couple of weak in-place volume-only transitions).
- **Against `uade123`, full mix, same 8s window**: reference `44` onsets (`5.5/s`) vs. this crate's
  `28` (`3.5/s`), inter-onset correlation `-0.108`. Confounded by the other 3 voices (uade123 has no
  solo flag), so read as corroborating, not conclusive on its own — but the direction (reference has
  *more* onsets) matches "we're swallowing notes that should restart," not the reverse.
- **User confirmed by ear** on the isolated render (`ours-pattern82-isolated.wav`): "some notes merge
  into others" instead of a clean five-note rhythm — matches the swallow/restart pattern found by
  instrumentation exactly.

### Open question: what's actually wrong, the heuristic or the latency it's racing against

Not yet settled which side of the race is miscalibrated:

1. **The `dma_on`-based heuristic itself may be the wrong invariant** for any cadence that isn't
   uniformly faster or slower than the attack latency — i.e. it was only ever validated against the
   two uniform extremes (macro 41: always-swallow-correct; macro 38: always-restart-correct), never
   against a pattern like 82 that straddles the boundary note-to-note. A per-retrigger race on a
   single-jiffy-resolution flag may not be what real hardware does at all.
2. **The 2-jiffy attack-latency figure itself may be wrong** — if `$00 aa=0`'s "mandatory 1-jiffy
   pause" or `$08`'s own suspend is miscalibrated (even by one jiffy), every dispatch in pattern 82
   would consistently find `dma_on` already true and always restart, which is what the `uade123`
   onset-count evidence above would also predict. This would point at `docs/playback-model.md` §2.4
   or the `$00`/`$08` opcode handlers, not at `note_on` at all.
3. Both could be partially true. **Do not guess a fix without more evidence** — this heuristic is
   load-bearing for three other now-fixed, ear-confirmed, golden-hash-locked cases (macro 41 in this
   same module, macro 38 in `turrican 2 title (st)`, macro 8 in `turrican outside`); a change here
   risks reopening any of them silently (no lint finding would catch a wrong-but-plausible retrigger
   decision the way `sample-region-out-of-bounds` caught §5).

### For whoever picks this up next

- Get editor ground truth for pattern 82 specifically (audition it directly, not just macro 28 alone
  — §9's Recipe A already showed macro 28 auditions differently than it plays inside this pattern).
  Does the real editor produce 5 distinct attacks, or does it also merge some notes the way our render
  currently does even less than uade123 suggests?
- Before changing `note_on`, re-derive `$00`/`$08`'s real suspend timing from `docs/playback-model.md`
  §2.4 and the opcode table (`docs/opcodes.md`) with fresh eyes — theory 2 above is cheaper to falsify
  than theory 1 (it's a local, single-opcode question, not a heuristic redesign) and would explain the
  evidence just as well.
- Whatever fix is chosen needs regression tests pinning **all four** now-known cases at once (macro
  41's always-swallow, macro 38's always-restart, macro 8's envelope-gated sustain, and pattern 82's
  mixed cadence) so a future change can't silently break one while fixing another — this thread's
  repeated failure mode.
- Rendered A/B files for this session are in the scratchpad (not committed): `ours-pattern82-
  isolated.wav`, `ours-fullsong-{mix,voice0}-p82window.wav`, `uade-full-p82window.wav`.
