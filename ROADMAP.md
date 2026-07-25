# Roadmap

> ## ▶ Status
>
> | | |
> |---|---|
> | **Next step** | **2.1 · `Module::parse`** (see Phase 2 below) |
> | **Phase** | 2 of 6 — Parser |
> | **Gate** | ⛔ Phase 1 **complete**. Phase 2 **not yet approved**. Ask before starting. |
> | **Last done** | 1.4 · `docs/architecture.md` — Phase 1 done |
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

## M1 — Documentation, parser, 4-channel renderer, WAV output

### Phase 0 — Repository, project setup, test corpus ✅

- [x] **0.1** Create the public GitHub repo `tfmx-rs`, set as `origin`, branch `main`. *(Haiku 4.5)*
- [x] **0.2** Cargo workspace: `tfmx` (dependency-free core) + `tfmx-cli` stub, dual
      MIT/Apache-2.0, README with the provenance note. — *check: `cargo build`* *(Haiku 4.5)*
- [x] **0.3** Test corpus via `testdata/fetch.sh`, audio excluded from git. — *check: 10
      mdat/smpl pairs, nothing tracked* *(Haiku 4.5)*

> Finding from 0.3: layout detection is a **plain zero check**, not a heuristic. The three
> longs at `$1D0` are either all zero (fixed-address layout) or all plausible ascending
> in-file offsets (packed). The corpus splits 5/5, so neither parser path can rot unnoticed.

### Phase 1 — Documentation (before any player code) ✅

Every step in this phase draws on [S1] and [S2] from [Sources](#sources), and on nothing else.

- [x] **1.1 · `docs/format.md` — the data model** *(Sonnet 5)*
      Byte-level layout of `mdat`/`smpl`. Header fields with offsets. The 96-word table (32
      song starts / 32 ends / 32 tempos). Both layout variants and how to tell them apart.
      Trackstep record layout. Pattern longword encoding (top-two-bits classification). Macro
      longword encoding. Sample representation (signed 8-bit, one-shot vs. loop). Big-endian
      throughout.
      *Diagrams:* ASCII header byte table; Mermaid pointer graph (header → `$1D0` table →
      trackstep / pattern / macro pointers → data), both variants; bit-field diagrams for the
      trackstep word, pattern longword and macro longword.
      *Check:* every stated offset traceable to a quoted line of the spec.

> Finding from 1.1, verified against all 5 fixed-layout corpus files: **[S1]'s fixed-layout
> offsets `$600,$200,$400` are wrong.** `$200`–`$3FF` is entirely zero; the real tables are
> pattern pointers at `$400`, macro pointers at `$600`, trackstep at `$800` — the spec's triple
> shifted `$200` low. `$400` is corroborated by [S1]'s own §3 fallback sentence. Step 2.2 must
> use `$400/$600/$800`. See `docs/format.md` §3.2.

- [x] **1.2 · `docs/opcodes.md` — complete command reference** *(Sonnet 5)*
      Three tables transcribed in full: trackstep `$EFFE` commands, pattern commands
      `$F0`–`$FF`, macro opcodes `$00`–`$21`. Columns: opcode, mnemonic, operand layout,
      effect, confidence (documented / inferred / unknown). Opcodes `$22`–`$29` and the `$FE`
      ambiguity get an explicit "Unresolved" section.
      *Diagrams:* operand-layout diagrams for `0B` portamento, `0C` vibrato, `0F` envelope,
      `18` sampleloop, `1E` addvol+note.
      *Check:* no gaps in the opcode ranges; every entry has an effect or sits in Unresolved.

- [x] **1.3 · `docs/playback-model.md` — how sound is produced** *(Sonnet 5)*
      The chain trackstep → pattern → macro → Paula registers → mixer. Paula voice semantics:
      period → frequency (`3_546_895 / period`), volume 0–64, DMA on/off, one-shot-then-loop,
      the DMA restart delay quirk. Timing: the jiffy, the 50 Hz divider path vs. the CIA path,
      why 24 jiffies make a beat. Note table, transpose + detune → period. Envelope, vibrato,
      portamento maths. **Plus a Gotchas section** (tempo is the classic failure; `$80` hold
      still applies transpose; `18` sampleloop is not idempotent; `1A` wait-on-DMA needs mixer
      state fed back; offsets are absolute into `mdat`; input is untrusted).
      *Diagrams:* Mermaid flowchart of the signal chain with the DMA feedback edge drawn; a
      state diagram of one voice's DMA lifecycle; a timeline of ticks inside a render block.
      *Check:* someone who has not read the spec could implement the timing from this alone.

> Finding from 1.3, verified across all 229 `$F1`/`$F2`/`$F8` commands in the corpus: **there are
> two distinct offset spaces.** Pointer-table entries and sample offsets are absolute byte
> offsets; jump/loop/gosub targets (`$F1`/`$F2`/`$F8`, `$05`/`$06`/`$15`, `$1C`/`$1D`) are
> **longword step indices relative to the enclosing pattern or macro**. No target reaches its own
> pattern's length, and odd targets rule out byte offsets. Step 4.3 must not treat them alike.

- [x] **1.4 · `docs/architecture.md` — the code shape** *(Sonnet 5)*
      Crate layout, the register seam and why it sits there, the `render()` contract,
      threading/allocation rules (neither inside the core), the public API, and the deliberate
      simplifications with their upgrade triggers.
      *Diagrams:* Mermaid module dependency graph with the seam marked; sequence diagram of one
      `render()` call.
      *Check:* names in the doc match the planned module names.

> Finding from 1.4: the roadmap named `Paula::render()` but never named the type that owns tick
> scheduling, leaving it unclear which `render()` step 4.1's block-size-independence check tests.
> `docs/architecture.md` resolves this with **two `render()`s at different levels**:
> `Paula::render(smpl, out)` mixes one chunk against constant register state (step 3.2's target),
> and the newly introduced **`Player`** wraps tick scheduling around it (step 4.1's target).
> `Sequencer` owns the tempo fraction; `Player` owns the accumulator phase that must survive
> across calls. Other new names: `AccessError`, `UnsupportedOps`, `Paula::loop_completions`.

### Phase 2 — Parser

- [ ] **2.1** `Module::parse(mdat, smpl) -> Result<Module, ParseError>`: magic, text area,
      96-word table. Borrowed slices, no sample copying. — *check: unit test asserts magic,
      song count, tempo values for a known file* *(Sonnet 5)*
- [ ] **2.2** Layout detection (zero check at `$1D0`) and resolution of the trackstep,
      pattern-pointer and macro-pointer tables. — *check: all 10 corpus files parse; detected
      variant matches `testdata/README.md`* *(Sonnet 5)*
- [ ] **2.3** Bounds-checked accessors `pattern(n)`, `macro_(n)`, `sample(offset, len)` — the
      trust boundary, never raw indexing. — *check: truncated buffer and corrupted offset table
      both return `Err` without panicking* *(Sonnet 5)*

### Phase 3 — Paula mixer

- [ ] **3.1** `Voice { start, len, period, volume, dma_on, loop_start, loop_len }` plus
      fractional position, and the setters the sequencer calls. *(Sonnet 5)*
- [ ] **3.2** `Paula::render()`: linear interpolation, one-shot → loop transition, hardware
      panning with the separation knob, volume scaling, clamped output. — *check: synthetic
      1000 Hz sine at a known period, count zero crossings, assert within 0.5 %* *(Sonnet 5)*
- [ ] **3.3** DMA state feedback: expose the loop-completion count for macro `1A`. — *check: a
      100-sample loop rendered for 10 loop lengths reports 10* *(Sonnet 5)*

### Phase 4 — Sequencer (the hard part)

- [ ] **4.1** Tick scheduling: `samples_until_next_tick`, the 50 Hz path and the CIA path,
      block-size independence. — *check: 1 second rendered as one 48000-frame call and as 480
      hundred-frame calls is bit-identical* *(Opus 5)*
- [ ] **4.2** Trackstep runner: `$EFFE` commands, `$80` hold with transpose, `$FF`/`$FE` stop,
      song start/end/loop. — *check: trace the first 200 ticks and verify against
      `docs/format.md`* *(Opus 5)*
- [ ] **4.3** Pattern decoder: longword classification, notes with detune, notes with wait,
      portamento, `$F0`–`$FF`. Triggers macros with channel, note, volume. — *check: pattern
      dump is self-consistent* *(Opus 5)*
- [ ] **4.4** Macro interpreter: per-voice PC and wait counters, note table, transpose + detune
      → period, envelope, vibrato, portamento, sample start/length/loop, DMA, one-shot,
      wait-on-DMA. Unknown `$22`–`$29` recorded, never guessed. — *check: 30 s render is not
      silent, no `NaN`/`inf`, no clipping, and bit-identical across two runs* *(Opus 5)*

### Phase 5 — CLI

- [ ] **5.1** `tfmx-cli render <mdat> <smpl> -o out.wav [--song N] [--seconds S] [--rate HZ]
      [--separation P]`. — *check: playable WAV of the requested length* *(Haiku 4.5)*
- [ ] **5.2** `tfmx-cli info`: header text, songs, tempos, detected layout, and the
      `unsupported_ops()` histogram. — *check: run across the corpus* *(Haiku 4.5)*

### Phase 6 — Verification and tuning

- [ ] **6.1** Golden-hash regression tests: SHA-256 of the first 10 s per module in
      `tests/golden.txt`, with a documented regeneration command. — *check: perturbing the
      volume scale makes it fail* *(Sonnet 5)*
- [ ] **6.2** A/B listening pass against a reference recording. Judge in this order: **tempo**
      (the classic bug), pitch, instrument attack, timbre. — *check: per-module notes in
      `docs/status.md`, remaining deviations listed rather than hidden* *(Opus 5)*

---

## Later milestones

- **M2 — Desktop realtime:** `cpal` output, play/pause/stop, song selection. The core does not
  change.
- **M3 — Web:** `tfmx-web` wasm-bindgen wrapper, AudioWorklet processor, minimal demo page with
  drag-and-drop for mdat/smpl.
- **Beyond:** TFMX 7V support (a separate parser path — the format is substantially different,
  not a flag), GemX macro opcodes, tools (pattern dump, sample export).

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
