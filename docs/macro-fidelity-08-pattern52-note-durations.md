# Macro/pattern fidelity: pattern 0x52/macro 0x1c (macro 28) — wrong note durations

**Status: root cause found, ear-confirmed, fix NOT chosen.** Distinct from [the macro 28 pitch saga](macro-fidelity-05-macro28-pitch-saga.md) (that thread is closed) — this is a separate bug in the same pattern/macro, found afterward. See "For whoever picks this up next" below for the two live options.

[← index](macro-playback-fidelity.md)

---

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

## Update 2026-08-04 (new session, after the doc split): editor ground truth obtained — real hardware always restarts all 5 notes

The user auditioned pattern 82 directly in the real TFMX editor (the "for whoever picks this up
next" item above): **it produces 5 distinct attacks**, every cycle, not a mix of restarts and
swallows. This settles the open question in `note_on`'s favor of being wrong somewhere — real
hardware never swallows a retrigger in this pattern, at any of its three gap lengths (2, 2, 4
jiffies as measured from each note's own predecessor).

**Re-examining the instrumented trace (`restart, swallow, restart, restart, swallow`) against this
new fact**: the swallow/restart pattern is not simply "gap ≤ 2 jiffies → swallow" — note 2 has the
same nominal 2-jiffy gap as note 1 (from its immediate predecessor's `Wait(1)`) but restarts, while
note 1 swallows. The difference: note 1's predecessor (note 0) genuinely restarted, resetting the
attack-latency clock, so note 1's dispatch really does land exactly 2 jiffies after a fresh
`trigger()` — a true boundary case. Note 2's predecessor (note 1) *swallowed* rather than
restarting, so the macro's `dma_on` state was already carried over from note 0's restart with an
*additional* 2 jiffies elapsed on top — 4 jiffies cumulative since the last real `trigger()`, safely
past the 2-jiffy latency, hence restart. So the current model's swallow decisions are only ever
exactly-on-the-boundary races (gap == modeled latency to the jiffy), never a clear miss — consistent
with **theory 2**: the 2-jiffy attack-latency figure itself is off by one. If the true latency is 1
jiffy (not 2), every dispatch in this pattern — including the boundary ones — would find `dma_on`
already true and always restart, matching the editor exactly.

**Not yet a safe fix**: shrinking the latency to 1 jiffy would put macro 41's case (1-jiffy retrigger
cadence, needs *always swallow*) exactly on the same boundary this session just showed is unreliable,
risking silently flipping its ear-confirmed behavior. Per item 2 above, this still needs the `$00`/
`$08` suspend-timing re-derivation before touching `note_on` or the opcode handlers — the editor
result narrows *which* theory is right, it doesn't yet supply a safe fix.

**Theory 2 re-derived from `docs/opcodes.md` directly, same session — does NOT hold up.** `$00`'s row
(`docs/opcodes.md:148`): "If `aa` = 0, the voice stops at the end of the play routine and **the voice
sequencer itself pauses for a jiffy**." `$08`'s row (`:156`): "**Ends macro processing for this
jiffy**." Both state unambiguously that the *macro program counter itself* halts, not merely a
DMA-hardware register — exactly what `MacroInterpreter::execute`'s `0x00`/`0x08` arms already
implement (`self.wait = Wait::Jiffies(0)`, suspending until the next jiffy boundary in both cases).
Tracing macro 28's shape (`$00 aa=0` → `$02`/`$02`/`$03`/`$0D` immediate → `$08` suspends → `$01
DMAon`) against `take_turn`'s state machine confirms the current 2-jiffy trigger→`dma_on` gap is
exactly what the documented per-opcode suspend rules produce, not an implementation slip. **This
rules out theory 2** — the attack-latency figure is correctly modeled. The evidence now points at
theory 1: `note_on`'s `dma_on`-based "still sustaining" heuristic is itself the wrong invariant for a
retrigger cadence that straddles the attack latency, not a timing bug elsewhere. Redesigning it still
carries the same regression risk flagged in item 3 above (macro 41/38/8) and needs
`docs/status.md`'s original rationale for that heuristic (predates this document) reviewed before
touching it — not yet done.
