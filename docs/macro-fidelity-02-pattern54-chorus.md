# Macro/pattern fidelity: pattern 0x54 / macros 0x30-0x31 (voice 2) chorus effect

**Status: mixed.** Tempo-mismatch report resolved as a comparison-methodology fact, not a bug. A second, separate, real bug (macro-internal pulse rate ~3.5x too slow) is confirmed but **PAUSED** pending a clock-domain investigation.

[← index](macro-playback-fidelity.md)

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

