# Macro/pattern playback fidelity: open items from a live editor cross-check

Started 2026-07-31, with the trackstep-timing fix (`docs/trackstep-timing-bug.md`) code-complete
but not yet confirmed by ear. While preparing for that listen, the user cross-checked individual
macros against the TFMX editor's own macro-audition feature and found real discrepancies,
independent of the trackstep-timing work. This document exists so a fresh session can pick the
investigation up without re-deriving the tooling or the findings below.

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
jiffy rate, not the song tempo). §6 is a resolved *tooling* gotcha (not an engine bug) found
while using the new `render-pattern`/`render-macro` isolation commands — read it before
re-reporting any "render-macro produces silence" symptom.**

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
and `$18`'s loop-length values (still open).
