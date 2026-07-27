# Roadmap

> ## ▶ Status
>
> | | |
> |---|---|
> | **Next step** | **Answer the TFMX editor's two questions, then finish the trackstep timing fix.** The "different melody" complaint has a root cause: `docs/opcodes.md` §2 states at *documented* confidence that `$F0 <End>` advances the trackstep, but `Player::run_jiffy` advanced the shared line pointer **unconditionally every tick**, on a comment that claimed to resolve `docs/playback-model.md` §7 and justified itself with "mostly `$80 Hold` words". A corpus count kills that premise: **5 of 10 modules contain zero `$80 Hold` words**, `turrican intro` among them, while the mean pattern runs 30-460 ticks and was given exactly **one**. Roughly **98% of the composed music never played** -- right instruments, wrong tune -- which also explains the 71% superseded-trigger rate, the all-voice silence gaps, the `no-retrigger` lint finding and the 4.4s loop. Fixed behind `TrackstepGate::{AllTracks, AnyTrack}` (`tfmx-cli render|trace --gate all|any`) because §7's aggregation question is genuinely unsettled; 5 tests, mutation-checked against both an always-advance and an over-strict mutation; the `track_pattern` shadow field deleted. **A second, coupled defect is now exposed and measured**: autocorrelating `uade123`'s 180s reference envelope gives six top periods that are *all* exact integer multiples of **0.64s = 32 jiffies at 50Hz**, but song 0's tempo is genuinely 3 (header tables verified aligned), which §3.2's cited `50/(v+1)` makes 12.5Hz -- incapable of that grid. **Not done**: unlistened, and the ten golden hashes are deliberately **not** regenerated. Full account in [`docs/trackstep-timing-bug.md`](docs/trackstep-timing-bug.md). |
> | **Previously** | **Step C round 2 is done: `note_period()`'s formula is FALSIFIED as the cause, but two real caller bugs were found and fixed -- the fix needs the user's ears (full mix + per-voice stems, A/B against `uade123`) before it counts as done.** Hypothesis: a systematic error in `note_period()` (`tfmx/src/macro_interp.rs`) would give right rhythm and wrong pitch on every module, i.e. exactly the "different melody" symptom. Checked coverage first: its five existing tests are *all five of `docs/playback-model.md` §4's own worked examples*, so tests and implementation came from the same four points and could not catch a formula that fits them and drifts elsewhere -- 59 of 64 reachable notes untested, the 8-bit/16-bit detune combination untested, the pattern record's `dd` path untested. **Formula: falsified.** New sweep over all 64 notes against an independently derived expectation (iterating the literal 2^(1/12), deliberately never calling `powf`) plus the octave-halving invariant on all 52 pairs: **max deviation 0**, well inside the ±1 the doc itself leaves open on rounding. Both tests mutation-checked (perturbing the exponent divisor 12.0 -> 12.02, a 0.17% error, fails them) so they demonstrably have teeth. **Callers: two real bugs, both fixed TDD.** (1) The pattern note record's `dd` detune was decoded into `NoteTiming::Detune` and then **silently dropped** -- `dispatch_pattern_entry` destructured `timing` away with `..`; it now flows through `note_on` into the same `self.detune` slot `$21` already used (which also fixes a leak found in passing: a `$21` detune used to outlive its note and bleed into later ones). (2) `note_period(note, word23 as i16 + self.detune)` is an unguarded `i16` add over **raw module data** -- `word23 >= $7F81` plus a positive `$21` detune panics in debug, violating `tfmx/tests/mutation_robustness.rs`'s never-panic contract; now `saturating_add`. **Audible size of the fix, measured, not assumed**: traced every plausible song slot of all ten modules (`--seconds 90`) -- only 422 of ~150 000 executed notes carry a non-zero `dd`, every value between `+1` and `+11` (`+0.4%`..`+4.3%`, at most 3/4 of a semitone), seven modules unaffected entirely, `turrican intro` renders **byte-identical**, and all ten golden hashes are unchanged (no regeneration needed). So the fix is right but cannot be the "different melody" cause -- that would need tens of percent. **The investigation stays open**; per the standing rule this is not "done" until the user has listened. Full account in `docs/status.md`'s "Update (2026-07-26): Step C round 2 -- `note_period()` pitch mapping" section. |
> | **Before that** | Step C round 1: **hypothesis falsified, no code changed.** The user chose hypothesis 3 -- pattern/macro-number and track/voice mapping. Concretely testable form: `decode_pattern_entry` (`tfmx/src/sequencer.rs:519-535`) reads the `cv` byte as `volume: cv >> 4` / `voice: cv & 0x0F`, and `voice_of` (`tfmx/src/player.rs:20-22`) then masks that nibble with `& 0x03` -- both flagged **Uncertain** in their own doc comments, and any real `v` value of 4-15 would be silently wrapped onto the wrong voice on every module. Checked test coverage first: the `cv` *split* is pinned by `tfmx/src/sequencer.rs:1255-1345` and `:1774-1848` against `docs/format.md` §6's worked examples, but every `v` in them is already 0-3 and `voice_of` has no test at all -- the masking was untested guesswork. Then swept `tfmx-cli trace` over **every plausible song slot of all ten corpus modules** (`--seconds 90`), counting the raw pre-mask nibble of each executed `Note`: **111 846 note entries across the nine four-voice modules, zero with a nibble outside 0-3** -- the mask is a no-op on every module the user flagged, so this cannot be the "different melody" cause. **No code changed.** Two keepers: `apidya (title)` (the one TFMX 7V module, out of scope) uses exactly seven `v` values -- 0,1,2,4,5,6,7, never 3 -- which is the first real-data support for reading `v` as a channel selector at all; and a static `disasm --pattern 0..127` scan *does* show nibbles up to 15, but only in slots past the last pattern any song references (decoded garbage, not music data -- do not mistake it for a lead). Per the plan, exactly one hypothesis was tested and the session stops: whether to instrument `note_period()` or the `cv` split next is the user's call. Full account in `docs/status.md`'s "Update (2026-07-26): Step C -- pattern/macro-number and track/voice mapping -- **hypothesis falsified**" section. |
> | **Phase** | 11 of 11 (M4) — Diagnostics tooling — complete. Phase gate's chosen repair is a genuine partial fix, landed and committed; the underlying "different melody" complaint remains open. **Still stop for explicit approval before starting the next milestone.** |
> | **Gate** | M3 complete and approved. M4/Phase 11 (planned and approved 2026-07-26) delivered the diagnosis; the gate then chose to repair `turrican intro`'s confirmed bug next (`apidya (title)` remains separately explained as TFMX 7V, unsupported, `docs/architecture.md` §9). **One real bug found, fixed, and committed**: every macro in this module opens with `$00 aa=0` (mandatory 1-jiffy pause) and ends its note-setting opcode on another 1-jiffy suspend, so `$01 DMAon` needs two clear jiffies after a trigger. `dispatch_pattern_entry` called `MacroInterpreter::trigger()` (a full reset) for every `Note` event regardless of whether the same macro was already running on that voice — a fast note run retriggering the same macro every jiffy reset `step` back to 0 each time, so `$01` was never reached. Confirmed real (not benign) by an A/B against `uade123`: this crate's voice 1 was completely silent (`rms=0.0`) over the run's whole 0.5-1.5s span while the reference had continuous energy there. Fix: `MacroInterpreter::note_on` (`tfmx/src/macro_interp.rs`) — same macro number + still running (not yet `$07`-stopped) → update note/volume/transpose in place instead of `trigger()`; otherwise unchanged. Post-fix, `tfmx-cli lint` reports no findings for `turrican intro` (was `no-retrigger` + clipping), and all ten corpus golden hashes changed and were regenerated. **But the user reports the full render still sounds very different from `uade123` after this fix** — so this bug was real but is not the (whole) explanation for the original A/B complaint. **Song-number mismatch ruled out**: `uade123 -g` confirms subsong 0 is its default (matching our `--song 0`); this crate's own song table (`tfmx-cli info`) shows slot 0 = lines 75-129/tempo 3, and 27 of the 32 slots are an identical placeholder (`50/50`/tempo 5, confirmed by trace to decode as a bare `$EFFE 0000 Stop`) that both tools evidently stop enumerating at the same boundary (uade reports exactly 5 subsongs, 0-4). Cross-checked independently by note density: this crate's song-0 trigger rate (26.1/s) matches `uade123`'s measured onset rate (27.1/s) far better than song 1 (81.2/s) or song 2 (132.2/s) would. Full account in `docs/status.md`'s "Update (2026-07-26): `turrican intro`'s confirmed bug fixed" section. |
> | **Last done** | Implemented `$FD <Lock>` and `$FB <PPat>` pattern commands, `tfmx/src/player.rs`. Both were previously recognized-but-no-op (`$FA <Fade>` in the same bucket was already fixed in an earlier session). TDD: `Lock` arms a per-voice jiffy countdown that drops `Note` dispatches for that voice while non-zero. `PlayPattern` redirects another track to a new pattern; collected during the per-track dispatch pass and applied once after every track has run that jiffy -- this single-pass ordering alone reproduces `docs/opcodes.md` §2's "own track lower than target: next entry / otherwise: immediate" rule with no extra bookkeeping. Found and fixed a real bug surfaced by the `PlayPattern` integration test: `Sequencer::track` resolves a `$80 <Hold>` word using its *own* remembered pattern number, independent of any pattern-level jump, so the existing reload check (`patterns[i].pattern() != number`) silently undid the jump on the next Hold; fixed with a new `Player::track_pattern` field tracking what the trackstep itself last assigned, separate from the live `PatternRunner`. `PlayPattern`'s own `transpose` field is decoded but deliberately left unapplied (documented gap: no source states which of the pattern-level transpose vs. the trackstep's own per-jiffy transpose wins). Six new unit/integration tests, all pass; full workspace suite, clippy, and all ten corpus `lint`/golden-hash checks unaffected (neither opcode appears in the corpus outside `apidya (title)`'s 75 `Lock` calls, which render byte-identical before/after -- no competing note ever lands in one of its lock windows). Not part of the M1-M4 step list; a real gap found during this investigation, done ad hoc like the earlier master-volume-slide addition. |
>
> Update this block in the same commit that ticks a checkbox.

Progress tracker and the authoritative step list. **Tick a box in the same commit that
completes the step.** `git log` records what happened; this file records what is next.

Each step names its deliverable, its verification, and the **minimum model** recommended to
implement it. "Minimum" means a smaller model is likely to get this subtly wrong; a larger
one is always fine.

## Working agreement

- Working language for code, comments, docs, commit messages and file names: **English**.
- One commit per completed step. Subject names the step, e.g. `docs: add format.md (step 1.1)`.
- **Stop at the end of every phase** and wait for explicit approval before starting the next.
- Documentation carries diagrams: Mermaid for processes and state machines, ASCII tables for
  on-disk byte layouts.
- **No GPL source is ever read.** Every existing TFMX replayer is GPL-2.0; this code is written
  from the published spec. Reference players are executed as black boxes for A/B listening
  only. See the provenance section of [README.md](README.md).

## Delegating a step

Each step below is written to be handed over **verbatim and on its own**. An agent working a
step gets exactly this and nothing more:

1. The step's own block (deliverable, diagrams, check) — not the other steps, not the phase
   history, not the conversation that led here.
2. The relevant entries from [Sources](#sources), and the already-written files in `docs/`
   that its step builds on.
3. The hard rules that bind it: **never read GPL replayer source**, English only, and for core
   code — no dependencies, no allocation after load, no I/O, no threads.
4. Its verification criterion, and the instruction to run it before reporting done.

Do not pass this file wholesale, the plan history, or prior steps' reasoning. An agent that
needs the roadmap to understand its task has been given a task that is not yet well specified
— sharpen the step instead of widening the context.

---

## Completed milestones

M1 through M4 are done. Their full step-by-step detail — deliverables, verification criteria,
design decisions and the `Finding from X` notes recorded along the way — moved to
[ROADMAP-history.md](ROADMAP-history.md) so routine work on remaining/future topics doesn't load
it by default. Pull it up when a step's rationale or a past gotcha is actually needed (e.g. before
touching the trace seam, the trackstep-advance timing, or `note_period()`/detune handling again).

- **M1** — Documentation (`docs/format.md`, `docs/opcodes.md`, `docs/playback-model.md`,
  `docs/architecture.md`), parser, Paula mixer, sequencer, `tfmx-cli`, golden-hash regression
  tests, A/B listening pass. Phases 0–6.
- **M2** — `tfmx-play`, a `cpal`-based desktop realtime player with transport controls. Phase 7.
- **M3** — `tfmx-web`, a `wasm-bindgen`/`AudioWorklet` web player with a no-bundler demo page.
  Phases 8–10.
- **M4** — Diagnostics tooling: voice mute/solo/stems, `TraceEvent`/`render_traced`, `tfmx-cli
  trace` and `lint`, mutation-robustness fuzzing. Phase 11. Plus ad hoc fixes found along the way
  (`$FB`/`$FD` pattern commands, `turrican intro`'s macro-retrigger bug, `note_period()` caller
  bugs) — all detailed in the history file. The diagnosis this milestone produced fed directly into
  the still-open investigation described in the Status block above.

---

## Later milestones

- **Export and static analysis** (the natural round after M4 — all of it wants a *static* module
  walker that resolves a song to its reachable patterns, macros and sample regions, rather than M4's
  runtime seam): machine-readable module dump, sample export (the `smpl` file has no directory —
  regions exist only as `$02`/`$03`/`$18` operands inside macros, so this is an analysis job, not a
  file split), round-trippable text disassembly (assemble → identical `mdat` would be the strongest
  parser validation this project could have), MIDI/SFZ export, piano-roll and structure diagrams,
  cross-module sample fingerprinting.
- **Beyond:** TFMX 7V support (a separate parser path — the format is substantially different,
  not a flag), GemX macro opcodes, tracker TUI in `tfmx-play`, web visualizer in the M3 demo.

## Sources

Cite these by tag when briefing a step.

| Tag | Source | What it gives |
|---|---|---|
| **S1** | [libxmp `docs/formats/tfmx-format.txt`](https://github.com/libxmp/libxmp/blob/master/docs/formats/tfmx-format.txt) — J. H. Pickard, *The TFMX Professional 2.0 Song File Format* | The authoritative spec: header layout, trackstep, pattern and macro opcode listings, the note table |
| **S2** | [RetrovertApp/playback-tfmx `TFMX.md`](https://github.com/RetrovertApp/playback-tfmx/blob/master/TFMX.md) | Background on 7V, a worked macro dump, scope notes. **Prose only — the surrounding repo is GPL-2.0, do not read its code.** |
| **S3** | [ExoticA wiki: TFMX](https://www.exotica.org.uk/wiki/TFMX) | Context, module inventory |
| **S4** | [VGMPF: MDAT](https://www.vgmpf.com/Wiki/index.php?title=MDAT) / [SMPL](https://www.vgmpf.com/Wiki/index.php?title=SMPL) | Short format overviews, cross-check only |
| **S5** | `testdata/` (fetch with `sh testdata/fetch.sh`) | 10 real modules, 5 packed / 5 fixed layout |

## Known risks

| Risk | Mitigation |
|---|---|
| Timing model (CIA timer vs. 50 Hz divider) — the most common failure, and it sounds like a working player at the wrong speed rather than like a bug | 50 Hz path first, CIA path added separately and verified by ear |
| Two header layouts | Both from the start; detection is the zero check found in 0.3 |
| GPL contamination | Reference players are executed, never read |
| Aliasing from naive resampling | Accepted for M1, marked with a `ponytail:` comment |
