# Macro/pattern fidelity: shared tooling reference (render-macro, render-pattern, gotchas)

Tooling added during the macro-playback-fidelity investigation, factored out here since every issue doc above references it. Two resolved tooling gotchas (not engine bugs) are included at the end — read them before re-reporting a "render-macro produces silence" symptom.

[← index](macro-playback-fidelity.md)

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

