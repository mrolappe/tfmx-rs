# M5 session log

Append-only log for the "Export and static analysis" milestone (see
[`docs/m5-plan.md`](m5-plan.md)). Each entry: what was done, problems hit, **mistakes made and
how they were resolved**, and anything a future session would otherwise have to re-derive.
Write it honestly — wrong turns are the valuable part. One entry per phase, in phase order.

## Phase 5.0 — Idea ledger and session log

Wrote `docs/analysis-tooling-ideas.md` from `m5-plan.md`'s decision table, phase list and
"Deferred to the ledger" list, and this file's skeleton. Both linked from `CLAUDE.md`'s "Where
the knowledge lives". No code changed, no problems hit.

## Phase 5.1 — Reference register-log spike (WinUAE Memwatch)

**Positive result: the mechanism works.** WinUAE 6.1.0's debugger (Shift+F12 to enter) sets a
logonly memory watch with `w <slot> <addr> <len> <R/W/I> <F/C/L/N>`, e.g.
`w 0 dff0a0 40 W L` watches all of `$DFF0A0-$DFF0DF` for writes without breaking. Each hit
prints one line to the debugger's own console: `Memwatch <slot>: break at <addr>.<size> <RWI>
<value> PC=<pc> <accessmask> (<reg>)`. Decodes cleanly onto `AUD0-3 LCH/LCL/LEN/PER/VOL` by
`addr & 0xFF`, base `0xA0`/`0xB0`/`0xC0`/`0xD0` per channel, `+0x0/+0x2/+0x4/+0x6/+0x8` per
register.

**Wrong turn #1: stdout redirection does not capture this.** First attempt launched WinUAE
from a terminal with `> file.log 2>&1`, expecting the debugger console to go through the same
stream (the app's *general* logging does — confirmed separately when an accidental plain
launch printed ROM-scan messages to stdout). It doesn't: the log file only ever received the
one `write_log()`-based "watchpoint set" confirmation line. Read WinUAE's own source
(`debug.cpp` on GitHub, `tonioni/WinUAE`) to find out why: `memwatch_hit_msg()` prints hit
events via `console_out_f()`, a separate stream that only writes to the in-app debugger console
window/buffer — never through `write_log()`. There's no in-debugger toggle to redirect it.
**Workaround: copy the console text out by hand** (select-all + copy in the console window,
paste to a file) after a playback run. Fine for a spike; not something Phase 5.2+ can automate
without further work on capture (out of scope for this milestone as currently planned — noted
here so a future session doesn't have to re-discover the `console_out_f` vs `write_log` split).

**Wrong turn #2 (partial): the hit-message format has no timestamp at all.** Also confirmed by
reading `debug.cpp` — `mwhit`/`memwatch_hit_msg` carry address, size, R/W/I, value and PC only;
no vpos/hpos/frame counter. **Fix: add a second logonly watch on `$DFF09C` (INTREQ)**, e.g.
`w 1 dff09c 2 W L`. AmigaOS clears the VERTB interrupt-request bit (`0x0020`) once per 50 Hz
frame at a fixed PC in this build (`$00FC1354`); counting those hits in the same interleaved
console stream gives jiffy-resolution relative timing for free. Verified on a ~13.5s capture:
675 VERTB markers, spacing consistent with a steady 50 Hz source (line-gap between markers
varies with how many `AUD*` writes fall in that frame, not with the marker itself becoming
irregular).

**Decoded sample** (from a `the_house_of_techno` capture the user confirmed sounded correct by
ear; `jiffy` derived from the VERTB marker count, 0-indexed from the first captured registers):

```
jiffy=0  AUD0LCH  0x0002  PC=00C28FBE
jiffy=0  AUD0LCL  0xEB24  PC=00C28FBE
jiffy=0  AUD0LEN  0x1000  PC=00C28FC8
jiffy=0  AUD0PER  0x0168  PC=00C29162
jiffy=1  AUD0VOL  0x0020  PC=00C28EF0
jiffy=2  AUD0LCH  0x0002  PC=00C29360
...
```
4691 `AUD*` writes decoded across 673 jiffies from one capture. **Caveat, worth carrying
forward**: the value field is sometimes wider than a real 16-bit chip register should allow
(e.g. an `LCL` write showing `0x00023B71`) — appears to be upper garbage bits passed through by
WinUAE's memory-access hook on what is actually a word write; mask to the low 16 bits when
parsing. Also, the first ~16 events on this hardware are a `0xFFFE`/`0x0000` sweep pattern at a
PC outside the player's normal cluster — looks like a POST/init diagnostic, not music; anyone
resuming should skip past it rather than treat it as a decode bug.

**Also surfaced, not investigated further this phase**: the user reports WinUAE 6.1.0's own
playback of the TFMX editor is sometimes audibly wrong compared to fs-uae, and sometimes
correct, on the same module — nondeterministic or state-dependent in a way not yet understood.
The capture above is from a run confirmed correct by ear. **This is a real risk to the oracle's
trustworthiness** if pursued further: a register log captured during one of the "wrong" runs
would encode WinUAE's own bug, not ground truth, with no independent way (yet) to tell which
kind of run produced a given log. Not chased down in this timebox; flagging it for whoever
re-decides how far oracle work goes next.

No `tfmx`/`tfmx-cli`/`tfmx-analysis` code was written or changed this phase — a spike-only
parser (Python, ad hoc) was used to produce the decoded sample above and was not committed, per
the phase's "don't prematurely build into the crate structure" instruction. Raw captured logs
(WinUAE console text) were kept outside the repo (session-local), consistent with this
project's existing practice for other rendered/captured artifacts derived from the copyrighted
test corpus.

## Phase 5.2 — Static walker core (`tfmx-analysis`)

**User's re-decide call (session 16, before this phase started)**: treat the 5.1 spike's
positive result as sufficient and move straight to the static walker, rather than spending more
of this milestone automating Memwatch capture or chasing the WinUAE-vs-fs-uae playback
inconsistency. Both stay open, unblocked, for a future session that wants the register-log
oracle specifically.

**Two small, justified additions to the `tfmx` core** (not scope creep — both are existing
private logic the walker needed exposed, no new behavior):

- `Module::pattern_offset(n)` / `Module::macro_offset(n)`: the absolute `mdat` byte offset a
  pattern/macro's data starts at. `Module::pattern`/`macro_` only ever returned the byte slice,
  not where it began — fine for every existing consumer (they all just read forward from
  offset 0 of the slice), but the provenance map needs the absolute start to report a byte
  span. Refactored `pointer_table_entry` into `pointer_table_offset` (returns the `u32`) so
  both accessors and the new offset methods share one bounds-checked lookup. TDD'd against the
  same known corpus entries the existing `pattern_and_macro_access_known_file` test uses.
- `sequencer::decode_line` made `pub` and re-exported as `tfmx::decode_line`, mirroring
  `decode_pattern_entry`'s existing seam (stateless decode, no execution-state context) — the
  walker needed a way to turn a raw trackstep line's 16 bytes into `TrackstepLine` without
  pulling in `Sequencer`'s stateful trackstep runner.

**Design choice: the walker does not execute control flow, it lists linearly to the
terminator — same shape as `tfmx-cli disasm`.** Patterns are scanned from step 0 to
`$F0 End`/`$F4 Stop` (or a 256-step cap, mirroring `disasm`'s `MAX_DISASM_STEPS`); macros from
step 0 to `$07 STOP`. `$F1 Loop`/`$1C Splitkey`/`$1D Splitvol` branches are not followed —
their operands are read (so a `Jump`/`GoSub`/`PlayPattern` target pattern, or a `$06 Cont`/
`$15 Go submacro`/`$21 Play macro` target macro, is still queued as reachable) but the walk
does not jump to the branch target's *step*; every referenced pattern/macro number gets its
own from-step-0 scan when it's popped off the worklist. This means a pattern only ever reached
via `Jump{step: 40}` still gets scanned from step 0, not step 40 — an approximation, but the
same one `disasm` already makes, and it errs toward *more* provenance coverage, not less. Not
revisited this phase; would need real per-track program-counter simulation (closer to
`PatternRunner`/`MacroInterpreter`) to do exactly, and that is out of Phase 5.2's scope per
`m5-plan.md`.

**Sample-region tracking is best-effort, not zone resolution.** `$02 SetBegin`/`$03 SetLen`/
`$11 AddBegin` (only its `aa == 0` one-shot form — the oscillating `aa != 0` vibrato form isn't
resolved to a static offset)/`$12 AddLen`/`$18 Sampleloop`/`$19 Set one shot sample` update a
small `SamplePointer` struct that mirrors `macro_interp.rs`'s own bookkeeping (same absolute-
`$02`, halved-`$18`-delta units as the two macro-fidelity fixes already landed on `main`), and
every touch snapshots the "live" region (loop region once `$18` has run, else the plain sample
region) into `WalkResult::sample_regions`. This does **not** attempt `$1C`/`$1D` interval
splitting into note/velocity zones — that is Phase 5.3's job, the milestone's stated "spine".

**Corpus result** (`walk_song(module, 0)`, all 10 corpus modules, song 0 only):

```
turrican intro: 53 patterns, 25 macros reachable; provenance 6072/19108 bytes (31.8%)
turrican outside: 29 patterns, 8 macros reachable; provenance 2176/12252 bytes (17.8%)
r-type: 37 patterns, 14 macros reachable; provenance 2432/7432 bytes (32.7%)
x-out (title): 27 patterns, 10 macros reachable; provenance 4004/9116 bytes (43.9%)
turrican 2 title (st): 61 patterns, 39 macros reachable; provenance 8344/20340 bytes (41.0%)
turrican 2 level 1-desert: 48 patterns, 13 macros reachable; provenance 3572/13024 bytes (27.4%)
turrican 2 level 3-flight: 32 patterns, 14 macros reachable; provenance 3524/14328 bytes (24.6%)
turrican 3 level 1: 28 patterns, 11 macros reachable; provenance 6768/16732 bytes (40.4%)
apidya (title): 43 patterns, 22 macros reachable; provenance 4300/7056 bytes (60.9%)
apidya (level 1): 9 patterns, 10 macros reachable; provenance 1364/8148 bytes (16.7%)
```

`apidya (title)` is the only module whose raw voice nibbles include 4-7 with no 3 — the 7V
signature holds across the whole corpus, asserted in
`walker::tests::walks_all_corpus_modules_song_0_without_panic`. Coverage is deliberately far
from 100% and not treated as a bug: only song 0 is walked (most modules carry more than one
song slot, unexplored this phase), and header/pointer tables themselves are structural data,
never claimed by any pattern/macro span — matching the phase's own interpretation note in
`m5-plan.md` ("the signal is the delta across modules, not 100%").

No mistakes hit worth recording as wrong turns this phase — the two `tfmx` accessor additions
and the walker itself passed their tests on the first real corpus run once the one seeded test
bug (a hand-encoded `cv` byte with volume/voice nibbles swapped in
`reachable_patterns_and_macros_from_trackstep`) was caught by its own assertion and fixed.

## Phase 5.3 — Zone resolution (`$1C`/`$1D`)

Delegated to an Opus 5 agent (self-contained brief: opcode semantics from `docs/opcodes.md`,
the runtime `$1C`/`$1D` reference in `tfmx/src/macro_interp.rs`, the existing walker to extend).

New `tfmx-analysis/src/zones.rs`: `resolve_zones(module, macro_number) -> ZoneTable`, a
symbolic pass that interprets a macro's `$1C <Splitkey>`/`$1D <Splitvol>` branches over
intervals rather than concrete values, partitioning the whole `0..=$3F` (note) x `0..=64`
(entry volume) rectangle into disjoint zones, each carrying its live sample region, volume
register and envelope.

**The interval algebra**: DFS over paths, each carrying a `(note interval, entry-volume
interval)` rectangle plus accumulated state; splits cut the rectangle and empty halves are
pruned. `$1C` cuts the note axis directly, since no macro opcode ever writes the note register.
`$1D` compares the volume *register*, already touched by `$0D`/`$0E`/`$1E`, tracked as
`clamp(entry + offset, lo, hi)` -- three fields, not a single accumulated offset, because
clamping does not compose with addition (`$0D -10` then `$0D +10` leaves entry-volume 0 at 10,
not 0). `$0F <Envelope>` (time-varying volume) or a revisited step yields `ZoneExit::Unresolved`
rather than a guess. `walk_song` and its tests are untouched; `SamplePointer`/`sext24` widened to
`pub(crate)` for reuse rather than re-derived.

New fixture: `testdata/synth/gen_split_probe.py` (+ generated `mdat`/`smpl.split-probe`,
`testdata/synth/` un-ignored) -- a from-scratch macro with one `$1C` threshold, for a
known-boundary test independent of the real corpus.

**Check results**: `turrican intro` macro 28 (no `$1C`/`$1D` in its disasm) resolves to exactly
one full-rectangle zone matching that linear structure field-for-field; the probe macro resolves
to exactly two zones split at the right note boundary. Corroborating, not required by the check:
macro 24's real keysplit, macro 5's `$1D` chain, and a coverage test probing every macro of all
10 corpus modules to confirm every point lands in exactly one zone. 240 workspace tests pass
(9 new), clippy clean, `wasm32-unknown-unknown` build for `tfmx` unaffected (only `tfmx-analysis`
touched).

**Open finding, not acted on**: macro 5's `$1D` chain (`$0D +$15` then four `$1D`s at
`$20/$2A/$34/$3C`) reads as dead code (3 of 4 `Cont` targets unreachable) under the documented
"jump if volume < aa" polarity from `docs/opcodes.md:177`, but as a clean 5-way velocity-layered
fan-out under the reverse polarity ("jump if volume >= aa") -- suggestive the documented polarity
may be backwards. Not investigated further this phase since 5.3's check criterion is to match
*current* documented/runtime behavior, not to resolve fidelity questions; a fidelity thread issue
if picked up later. Test `zones::tests::turrican_intro_macro_5_splitvol_chain` documents today's
reading. Also noted: macro 28's `$0E <SetVolume>` has `aa=$00` (resolved volume 0, `cc=$38`
unused) -- mirrors the interpreter, not investigated.
