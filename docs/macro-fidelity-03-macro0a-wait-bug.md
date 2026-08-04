# Macro/pattern fidelity: pattern 0x6b / macros 0x0a, 0x27 — $04 Wait zero-jiffies bug

**Status: FIXED, TDD'd.** Structurally verified by [differential render](macro-fidelity-04-paula-handoff.md); audible pitch complaint later closed corpus-wide by [the MIDDLE_C_NOTE fix](macro-fidelity-05-macro28-pitch-saga.md).

[← index](macro-playback-fidelity.md)

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

