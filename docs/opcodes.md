# TFMX Opcode Reference

Complete command reference for the TFMX Professional 2.0 song format: trackstep
`$EFFE` line commands, pattern commands `$F0`–`$FF`, and macro (voice program)
opcodes `$00`–`$21`.

**Sources**: J. H. Pickard, *The TFMX Professional 2.0 Song File Format* (the
authoritative spec, cited as [S1] below), and the worked macro-dump example
in the `playback-tfmx` project README prose notes (cited as [S2], used only
to sanity-check operand layouts against a real macro). No replayer source
code was read; every existing TFMX replayer is GPL-2.0 and this crate is
written from the published spec so it can stay MIT/Apache-2.0.

## How to read this document

Every command lives in a 4-byte slot: one opcode byte followed by three
operand bytes, packed as a longword (`opcode aa bb cc`), *except* trackstep
`$EFFE` commands, which are word-addressed (see §1). In operand-layout
notation, borrowed directly from [S1]:

- `aa`, `bb`, `cc` — data bytes; their meaning is explained in the Effect
  column. Where two letters repeat (`bb bb`), the two bytes form one 16-bit
  value.
- `xx` — a don't-care / unused byte.
- A lower-case letter folded into a nibble (e.g. `xb`, `bv`) means the byte
  is nibble-packed: one nibble is don't-care, the other carries the named
  field.
- `v` — a voice/channel number, 0–F.

Hex values carry a `$` prefix throughout, matching [S1]'s own convention.

### Confidence key

- **documented** — [S1] states both the effect and the operand layout
  outright.
- **inferred** — worked out from context, from the [S2] macro dump, or by
  analogy with an adjacent/related opcode. The row says what the inference
  rests on.
- **unknown** — named in [S1] with no usable description, or not named
  anywhere in [S1].

Never read "inferred" as "documented" — an interpreter that guesses wrong on
these produces music at the wrong speed or with the wrong envelope, silently.
Where a row is unknown, the intended behavior is to record the opcode and
move on, not invent a plausible-sounding effect.

---

## 1. Trackstep line commands (`$EFFE`)

The trackstep is an array of lines, one per song step; each line holds eight
words, one per track. Normally each word's high byte is a pattern number and
its low byte a transpose (see the note on per-track word values below). When
the **first** word of a line is `$EFFE`, the whole line is reinterpreted as a
command instead of track data:

```
 word0     word1       word2 (line+4)   word3 (line+6)
+--------+-----------+-----------------+-----------------+
| $EFFE  | command # |     param A     |     param B     |
+--------+-----------+-----------------+-----------------+
```

The word after `$EFFE` selects the command; the meaning of the two
parameter words depends on which command was selected.

| Opcode (word1) | Mnemonic | Operand layout | Effect | Confidence |
|---|---|---|---|---|
| `$0000` | Stop | (no parameters) | Stops the player. | documented |
| `$0001` | PlaySection | param A = position, param B = times | Plays a section starting at `position` and ending at the current line, `times` times. `times` = `$0000` repeats forever. | documented |
| `$0002` | SetTempo | param A = divisor, param B = CIA bpm | Sets playback tempo. `divisor` > 15 is used directly as a beats-per-minute figure (one beat = 24 jiffies); `divisor` ≤ 15 divides a 50 Hz base rate (`0`=50 Hz, `1`=25 Hz, `2`=16.7 Hz, …). `CIA bpm` = `$FFFF` (-1) means "no change". | documented |
| `$0003` | MasterVolSlide (A) | param A = divisor, param B = target | Starts a master-volume slide toward `target`, one step every `divisor` jiffies. | documented — see note |
| `$0004` | MasterVolSlide (B) | param A = divisor, param B = target | Same stated effect and layout as `$0003`; [S1] describes both with a single shared sentence ("EFFE0003, EFFE0004 ... line+4=divisor, line+6=target") and never states what distinguishes the two opcode numbers (direction? fade curve? one-shot vs. repeating?). | documented — see note |

Note: for `$0003`/`$0004`, the layout and the general "start a master volume
slide" effect are directly stated by [S1], so they are marked documented —
but the spec gives no way to tell the two apart. Treat the distinction
between them as an open question when implementing; do not guess a direction.

### Per-track word values (context, not a command)

For completeness: when a line is *not* an `$EFFE` command, each of its eight
words is `pattern:transpose` (high byte : low byte), with three reserved
high-byte values — `$80` holds the last position (transpose still taken from
the low byte), `$FF` stops the track, and `$FE` stops the voice named in the
low byte. All three are stated plainly in [S1] §2 and are not ambiguous; they
are listed here only so the `$EFFE` table above isn't read as the whole of
trackstep encoding. (This `$FE` is unrelated to the pattern-command `$FE`
discussed in the Unresolved section — different opcode space entirely.)

---

## 2. Pattern commands (`$F0`–`$FF`)

Each pattern is a series of longwords; each longword is either a note or a
command, distinguished by the top two bits of the first byte:

```
 00: note, lsb = detune
 01: note, lsb = detune
 10: note, lsb = wait
 11: portamento-note (first byte $C0-$EF) or command (first byte $F0-$FF)
```

A note longword is `aa bb cv dd`: `aa` = note number (`$00`–`$EF`), `bb` =
macro to play it with, `c` = relative volume nibble (`$F` relative = `$2D`
absolute), `v` = voice nibble, `dd` = jiffies to wait before the next
command. If `aa` < `$80` the wait byte is instead a finetune value and the
next command is fetched immediately; if `aa` > `$BF` the note is reached by
portamento from the previous note (as `$FC` below) rather than played
directly. Only the low 6 bits of `aa` select the note.

Values `$F0`–`$FF` (the note byte's top six bits all set) are never note
data; they always dispatch to the table below. `v` in the table stands for a
voice number `0`–`F`; `x` is a don't-care byte.

| Opcode | Mnemonic | Operand layout | Effect | Confidence |
|---|---|---|---|---|
| `$F0` | `<End>` | `xx xx xx` | Ends this pattern; trackstep advances. | documented |
| `$F1` | `<Loop>` | `aa bb bb` | Repeats the block from `bbbb` up to (not including) this command, `aa` times. `aa` = `$00` repeats indefinitely. | documented |
| `$F2` | `<Jump>` | `aa bb bb` | Jumps into pattern `aa` at point `bbbb`. | documented |
| `$F3` | `<Wait>` | `aa xx xx` | Waits `aa`+1 jiffies. | documented |
| `$F4` | `<STOP>` | `xx xx xx` | Stops this track. Unrecoverable until a new pattern pointer is loaded; will not run any upcoming `<End>`. | documented |
| `$F5` | `<Kup^>` | `xx xv xx` | Sets a release flag on voice `v`. If that voice's macro program is waiting for release, it continues; otherwise no effect. | documented |
| `$F6` | `<Vibr>` | `aa xv bb` | Same effect as macro `$0C` `<Vibrato>`, applied to voice `v`; see §3. | documented — by explicit cross-reference in [S1] |
| `$F7` | `<Enve>` | `aa bv cc` | Every `b`+1 jiffies, slides voice `v`'s volume by `aa` towards `cc`. | documented |
| `$F8` | `<GsPt>` | `aa bb bb` | Saves the current pattern program counter, then behaves exactly as `$F2` (jump to pattern `aa` at `bbbb`). | documented |
| `$F9` | `<RoPt>` | `xx xx xx` | Restores the program counter saved by `$F8` and resumes execution there. | documented |
| `$FA` | `<Fade>` | `aa xx bb` | Every `aa` jiffies, slides the master volume by 1 towards `bb`. | documented |
| `$FB` | `<PPat>` | `bb xa cc` | Jumps track `a` to pattern `bb` with transpose `cc`, and continues. If this command's own track number is lower than track `a`, the jump takes effect on the next entry into the play routine; otherwise it is immediate. | documented |
| `$FC` | `<Port>` | `aa xv bb` | Every `aa` jiffies, multiplies voice `v`'s current period by `(256+bb)/256`. | documented |
| `$FD` | `<Lock>` | `aa bb bb` | Locks channel `aa`&3 against other notes for `bbbb` ticks. | documented |
| `$FE` | `<StCu>` | `xx xx xx` | Stated as "see `$F4`" (stop custom pattern) — see the [Unresolved](#unresolved) section; [S1] explicitly says what distinguishes it from `$F4` is unknown. | documented (base effect) / unknown (distinguishing behavior) — see Unresolved |
| `$FF` | `<NOP!>` | `xx xx xx` | Does nothing; pattern pointer advances to the next command. | documented |

---

## 3. Macro opcodes (`$00`–`$21`)

Macro data is a series of longwords, one macro program per voice, in the
same `opcode aa bb cc` shape as pattern commands. [S1] marks certain
commands with an asterisk (`*`) — meaning the command can suspend the
voice's macro program for one or more jiffies; that marker is preserved on
the mnemonic below exactly as in the source.

| Opcode | Mnemonic | Operand layout | Effect | Confidence |
|---|---|---|---|---|
| `$00` | `<DMAoff+Reset>*` | `aa xx xx` | Stops all effects and kills the voice. If `aa` ≠ 0, the voice stops immediately and the next command runs right away. If `aa` = 0, the voice stops at the end of the play routine and the voice sequencer itself pauses for a jiffy. | documented |
| `$01` | `<DMAon>` | `xx xx xx` | Turns on the voice's DMA channel. | documented |
| `$02` | `<SetBegin>` | `aa aa aa` | Adds the 24-bit value `aaaaaa` to the sample's base address and loads that into Paula. | documented |
| `$03` | `<SetLen>` | `xx aa aa` | Loads `aaaa` into Paula's length register (one count of `aaaa` = two bytes). | documented |
| `$04` | `<Wait>*` | `xx aa aa` | Waits `aaaa` jiffies. | documented |
| `$05` | `<Loop>` | `aa bb bb` | Plays the section from `bbbb` to here `aa` times, then continues past this command. | documented |
| `$06` | `<Cont>` | `aa bb bb` | Jumps into macro `aa`, starting at point `bbbb`. | documented |
| `$07` | `<STOP>*` | `xx xx xx` | Stops this channel's macro processing until a new note is invoked. | documented |
| `$08` | `<AddNote>*` | `aa bb bb` | Loads the current note, transposed by `aa` (and by the track transpose where applicable), into the period register. `bbbb` is a finetune value (`$0000`=100%, `$0080`=150%, `$FF80`=50%). Ends macro processing for this jiffy. | documented |
| `$09` | `<SetNote>*` | `aa bb bb` | Loads note `aa` (transposed where applicable) directly into the period register. `bbbb` is a finetune value as in `$08`. Ends macro processing for this jiffy. | documented |
| `$0A` | `<Reset>` | `xx xx xx` | Clears all effects: stops frequency/pointer vibrato, portamento, and volume slides. | documented |
| `$0B` | `<Portamento>` | `aa bb bb` | Every `aa` jiffies, multiplies the period by `(256+bb)/256`. If portamento wasn't already running, the current period is loaded in first as the starting point. See diagram in §4. | documented |
| `$0C` | `<Vibrato>` | `aa xx bb` | Every jiffy, slides the period by `bb`. The vibrato waveform starts on the rising zero-crossing of a triangle wave; `2×aa` is the waveform's period in jiffies. See diagram in §4. | documented |
| `$0D` | `<AddVolume>` | `xx xx aa` | Adds `aa` to the coarse volume set by the note-play command, ×3, and loads the result into the volume register. Corroborated by the [S2] macro dump, which shows a `$0D` step with operand `$000014` labeled "note/CONST./volume" against a plain add-volume-by-`$14` effect — consistent with this layout (see §4 note under `$1E`). | documented |
| `$0E` | `<SetVolume>` | `aa xx xx` | Moves `aa` directly into the volume register. | documented |
| `$0F` | `<Envelope>` | `aa bb cc` | Every `bb` jiffies, slides the volume by `aa` towards `cc`. See diagram in §4. | documented |
| `$10` | `<Loop key up>` | `aa bb bb` | Same as `$05`, but breaks out of the loop early if the key-up flag is set. | documented |
| `$11` | `<AddBegin>` | `aa bb bb` | Pointer vibrato. Each jiffy, for `aa` jiffies, adds `bbbb` to the sample pointer; after `aa` jiffies the direction reverses and the cycle repeats — unless `aa` = 0, in which case `bbbb` is added to the sample pointer exactly once, at the time of this command, and nothing further happens. | documented |
| `$12` | `<AddLen>` | `xx aa aa` | Adds `aaaa` to the loop length and stores the result in the length register. | documented |
| `$13` | `<DMAoff>*` | `aa xx xx` | Stops DMA without stopping effects; compare `$00`. | documented |
| `$14` | `<Wait key up>*` | `xx xx aa` | Waits `aa` cycles, or until key-up is received. `aa` = 0 waits indefinitely. | documented |
| `$15` | `<Go submacro>` | `aa bb bb` | Saves the macro program counter and jumps to macro `aa` at point `bbbb`. Compare `$06`. | documented |
| `$16` | `<Return to old macro>` | `xx xx xx` | Recalls the macro program counter saved by `$15` and resumes execution there. | documented |
| `$17` | `<Set period>*` | `xx aa aa` | Loads `aaaa` directly into the period register (absolute period). Ends sound processing for this jiffy. | documented |
| `$18` | `<Sampleloop>` | `aa aa aa` | Adds the 24-bit value `aaaaaa` to the sample start address, and subtracts the same value from the sample length. See diagram in §4. | documented |
| `$19` | `<Set one shot sample>` | `xx xx xx` | Loads the null sample into the appropriate registers. | documented |
| `$1A` | `<Wait on DMA>*` | `xx aa aa` | Plays the sample `aaaa` times, then continues with the next instruction. | documented — see note |
| `$1B` | `<Random play>` | unstated | [S1] names the command and writes only "?" for its effect — no operand layout or behavior is given at all. | unknown — see [Unresolved](#unresolved) |
| `$1C` | `<Splitkey>` | `aa bb bb` | Jumps to step `bbbb` in this macro if the current note is less than `aa`. | documented |
| `$1D` | `<Splitvol>` | `aa bb bb` | Jumps to step `bbbb` in this macro if the volume is less than `aa`. Intended for use after `<AddVolume>` to do velocity checks. | documented |
| `$1E` | `<AddVol+Note>*` | `aa $FE bb` | Performs an `<AddVolume>` with `bb` as its parameter, then an `<AddNote>` with `aa` as transpose. May suspend the macro program as `<AddNote>` does. See diagram in §4. | documented |
| `$1F` | `<SetPrevNote>*` | `aa bb bb` | Loads the last note, transposed by `aa` (and by track transpose where applicable), into the period register. `bbbb` is a finetune value as in `$08`. Ends sound processing for this jiffy. | documented |
| `$20` | `<Signal>` | `aa bb bb` | Loads `bbbb` into signal register `aa`&3. | documented |
| `$21` | `<Play macro>` | `aa xb cc` | Starts macro `aa` on channel `b` with detune `cc`. | documented |

Note on `$1A`: [S1] names this command `<Wait on DMA>` but its only stated
effect ("plays the sample `aaaa` times, then continues") describes a repeat
count, not a DMA-completion wait. The operand layout and the literal effect
text are both taken directly from the spec (hence "documented"), but the
name and the description do not obviously agree — implementers should treat
the *name* as suspect and the *stated behavior* as the contract to follow.

---

## 4. Operand-layout diagrams

Bit/byte-field diagrams for the five opcodes the roadmap calls out
specifically. Each command occupies one longword: opcode byte, then three
operand bytes, byte 0 first.

### `$0B` — Portamento (macro)

```
 byte 0     byte 1     byte 2      byte 3
+--------+----------+----------------------+
|  $0B   |    aa    |        bb bb         |
| opcode | rate:    |  portamento step     |
|        | apply    |  (16-bit, signed)    |
|        | every aa |  period *= (256+bb)  |
|        | jiffies  |            /256      |
+--------+----------+----------------------+
```
If portamento was not already running when this command executes, the
current period is loaded in first as the starting point (per [S1]).

### `$0C` — Vibrato (macro)

```
 byte 0     byte 1     byte 2     byte 3
+--------+----------+----------+----------+
|  $0C   |    aa    |    xx    |    bb    |
| opcode | half-    | unused   | per-jiffy|
|        | period:  |          | slide    |
|        | waveform |          | amount   |
|        | period = |          | (signed) |
|        | 2*aa     |          |          |
+--------+----------+----------+----------+
```
The vibrato waveform is a triangle wave that starts on its rising
zero-crossing.

### `$0F` — Envelope (macro)

```
 byte 0     byte 1     byte 2     byte 3
+--------+----------+----------+----------+
|  $0F   |    aa    |    bb    |    cc    |
| opcode | slide    | period:  | target   |
|        | amount   | every bb | volume   |
|        | per step | jiffies  |          |
+--------+----------+----------+----------+
```
Every `bb` jiffies, volume moves by `aa` towards target `cc`. (Same field
roles as pattern command `$F7 <Enve>`, minus the voice-number nibble — a
macro already runs on a fixed voice.)

### `$18` — Sampleloop (macro)

```
 byte 0     byte 1     byte 2     byte 3
+--------+---------------------------------+
|  $18   |            aa aa aa             |
| opcode |     one 24-bit signed value     |
+--------+---------------------------------+
```
Unlike the `aa bb bb` / `aa bb cc` opcodes above, all three operand bytes
here form a *single* 24-bit value, applied twice: added to the sample start
address, and subtracted from the sample length — i.e. it moves the loop
start forward (or back) by `aaaaaa` bytes while shrinking (or growing) the
remaining sample length by the same amount, so the sample's end point is
unchanged.

### `$1E` — AddVol+Note (macro)

```
 byte 0     byte 1     byte 2     byte 3
+--------+----------+----------+----------+
|  $1E   |    aa    |   $FE    |    bb    |
| opcode |transpose,|  literal |  volume  |
|        |fed to the| constant |  delta,  |
|        |internal  |  (not a  |  fed to  |
|        |AddNote   |  variable|  the     |
|        |          |  field)  |  internal|
|        |          |          |  AddVol- |
|        |          |          |  ume     |
+--------+----------+----------+----------+
```
[S1] writes byte 2's value literally as `FE`, unlike its usual `xx`
don't-care notation elsewhere in the table — this reads as a fixed constant
baked into the command's encoding rather than a meaningful data field. This
is unrelated to the pattern command `$FE <StCu>` discussed below; the two
occupy different opcode spaces (macro vs. pattern) and the shared byte value
is coincidental as far as [S1] states. Implementers should verify the
constant shows up as `$FE` in real module data before relying on it.

The [S2] worked macro dump does not exercise `$1E` directly — its example
achieves the same "add volume, then set note" sequence with two separate
commands (`$0D <AddVolume>` followed by `$08 <AddNote>`), which the dump's
prose labels "Addvol+note" as a human-readable description of the pair
rather than as the single fused opcode `$1E`. It was useful only to
corroborate `$0D`'s `xx xx aa` layout (see §3), not `$1E`'s.

---

## Unresolved

Opcodes and behaviors that [S1] does not pin down well enough to document as
fact. An implementation should record these as recognized-but-unimplemented
(or implemented behind an explicit guess, clearly marked as such) rather
than encode a silent assumption.

### Macro opcodes `$22`–`$29` — real-time sample manipulation

[S1] §4 states only: "22 through 29 are used in later TFMX players. They
perform real-time sample manipulation. These are most notably used in GemX.
Due to lack of research these are undocumented here." No operand layouts,
no per-opcode effects, no opcode-to-name mapping is given.

[S1] separately lists eight command-name strings pulled from a player's
string table, with the author's own caveat that he "picked these names at
random one drunken night" — i.e. even the author does not vouch for them:

```
MacrSIDSampleMsg   'SID setbeg  xxxxxx   sample-startadress'
MacrSIDLengthMsg   'SID setlen  xx/xxxx  buflen/sourcelen  '
MacrSID2OfsMsg     'SID op3 ofs xxxxxx   offset            '
MacrSID2VibMsg     'SID op3 frq xx/xxxx  speed/amplitude   '
MacrSID1OfsMsg     'SID op2 ofs xxxxxx   offset            '
MacrSID1VibMsg     'SID op2 frq xx/xxxx  speed/amplitude   '
MacrSIDFilterMsg   'SID op1     xx/xx/xx speed/amplitude/TC'
MacrSIDStopMsg     'SID stop    xx....   flag (1=clear all)'
```

There are exactly eight strings for the eight opcodes `$22`–`$29`, which is
suggestive, but [S1] never states an ordering or an opcode-to-string
mapping — so no row in §3 attributes a specific string to a specific opcode
number. Treat this list as an unordered pool of plausible names/operand
hints, not as a table. All eight opcodes are **unknown**.

### Pattern command `$FE` vs. `$F4`

[S1] §3 gives `$FE <StCu>` the description "See `$F4`. (What's special
about this is unknown.)" — meaning the spec's own author could not say what
distinguishes `$FE` from the already-documented `$F4 <STOP>` (stop track,
unrecoverable, does not run a pending `<End>`). Treat `$FE`'s base effect as
"same as `$F4`" (**documented**, by explicit cross-reference) but its
distinguishing behavior, if any, as **unknown**. Do not assume `$FE` and
`$F4` are simply aliases without confirming against real module playback.

(Not to be confused with the trackstep per-track word value `$FE`, "stop
the voice in the low byte" — see §1 — which is separately and
unambiguously documented, nor with the literal `$FE` byte inside macro
opcode `$1E`'s operand — see §4. All three are different namespaces that
happen to share the byte value `$FE`.)

### Macro opcode `$1B` — Random play

[S1] §4 lists `$1B <Random play>` with an operand-layout column left blank
and an effect description of a single question mark: "1B / Random play /
?". There is nothing in [S1] to infer an operand layout or effect from,
and no related opcode close enough in numbering or behavior to reason by
analogy. **Unknown** in full — record the opcode (recognize it, consume the
longword, do not attempt to execute it) rather than guess.
