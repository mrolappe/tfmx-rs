# Macro/pattern fidelity: portamento-to-note pattern records silently dropped

**Status: OPEN, not fixed.** Design question unresolved (see `For the next session` below).

[← index](macro-playback-fidelity.md)

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

