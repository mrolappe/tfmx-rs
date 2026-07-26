# Roadmap

> ## ▶ Status
>
> | | |
> |---|---|
> | **Next step** | **Implement master-volume slide (`$EFFE 0003`/`0004`, pattern `$FA <Fade>`) — currently a documented no-op — defaulting master volume to 0 at song start, then re-A/B `turrican intro` by ear.** A fresh stems-vs-`uade123` listen found this crate's voice 3 playing a drum-like hit almost immediately while the reference has no audible percussion until near the end; `turrican intro`'s trackstep line 75 (the song's first line) is exactly `MasterVolSlideB { divisor: 0, target: 64 }`, and this crate has no master-volume concept at all (`tfmx/src/sequencer.rs:347-353` already says so). Transpose plumbing, `note_period`, `$FB PlayPattern`, and `Portamento`-timed notes were all individually checked this round and ruled out for this module — full detail in `docs/status.md`'s "Update (2026-07-26, later)" section, including the TDD/default-value plan and the one open uncertainty (default master volume before any slide is not stated by `[S1]`; the original TFMX editor v1.5 was tried as an independent ground-truth source but would not play this module at all). **Do not start implementing until the user is ready to resume** — this was explicitly deferred to a fresh session. |
> | **Phase** | 11 of 11 (M4) — Diagnostics tooling — complete. Phase gate's chosen repair is a genuine partial fix, landed and committed; the underlying "different melody" complaint remains open. **Still stop for explicit approval before starting the next milestone.** |
> | **Gate** | M3 complete and approved. M4/Phase 11 (planned and approved 2026-07-26) delivered the diagnosis; the gate then chose to repair `turrican intro`'s confirmed bug next (`apidya (title)` remains separately explained as TFMX 7V, unsupported, `docs/architecture.md` §9). **One real bug found, fixed, and committed**: every macro in this module opens with `$00 aa=0` (mandatory 1-jiffy pause) and ends its note-setting opcode on another 1-jiffy suspend, so `$01 DMAon` needs two clear jiffies after a trigger. `dispatch_pattern_entry` called `MacroInterpreter::trigger()` (a full reset) for every `Note` event regardless of whether the same macro was already running on that voice — a fast note run retriggering the same macro every jiffy reset `step` back to 0 each time, so `$01` was never reached. Confirmed real (not benign) by an A/B against `uade123`: this crate's voice 1 was completely silent (`rms=0.0`) over the run's whole 0.5-1.5s span while the reference had continuous energy there. Fix: `MacroInterpreter::note_on` (`tfmx/src/macro_interp.rs`) — same macro number + still running (not yet `$07`-stopped) → update note/volume/transpose in place instead of `trigger()`; otherwise unchanged. Post-fix, `tfmx-cli lint` reports no findings for `turrican intro` (was `no-retrigger` + clipping), and all ten corpus golden hashes changed and were regenerated. **But the user reports the full render still sounds very different from `uade123` after this fix** — so this bug was real but is not the (whole) explanation for the original A/B complaint. **Song-number mismatch ruled out**: `uade123 -g` confirms subsong 0 is its default (matching our `--song 0`); this crate's own song table (`tfmx-cli info`) shows slot 0 = lines 75-129/tempo 3, and 27 of the 32 slots are an identical placeholder (`50/50`/tempo 5, confirmed by trace to decode as a bare `$EFFE 0000 Stop`) that both tools evidently stop enumerating at the same boundary (uade reports exactly 5 subsongs, 0-4). Cross-checked independently by note density: this crate's song-0 trigger rate (26.1/s) matches `uade123`'s measured onset rate (27.1/s) far better than song 1 (81.2/s) or song 2 (132.2/s) would. Full account in `docs/status.md`'s "Update (2026-07-26): `turrican intro`'s confirmed bug fixed" section. |
> | **Last done** | 11.6 · Mutation robustness test, `tfmx/tests/mutation_robustness.rs`: a hand-rolled 64-bit LCG (no dependency) flips one random byte in a clone of each corpus `mdat`/`smpl` buffer, 300 times per module across all 10 modules (3,000 mutations), and asserts `Module::parse` + a 1 s `Player::render` never panics (`Err` is accepted from either call; only a panic fails the test). Passes clean in ~13 s. **The step's own suggested negative control does not reproduce**: reverting the `number & 0x7F` mask in `sequencer.rs::decode_track_word` no longer makes the test fail, because `Module::pattern`/`pointer_table_entry` already rejects any out-of-range pattern number as `Err(AccessError::OutOfRange)` via `checked_add` + `slice::get` — every pointer-table and trackstep-line accessor in `module.rs` is bounds-checked the same way, and the mask is now a value-tolerance choice, not the panic guard. To confirm the harness itself still catches real regressions, `Module::trackstep_line`'s `.get(start..end)` was temporarily swapped for direct unchecked indexing (reintroducing the step-10.1 bug class), the mutation test failed immediately naming the exact module/mutation/byte, and the change was reverted. Full details in `docs/status.md`'s "Update (2026-07-26, step 11.6)" section. No change to non-test source. |
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

- [x] **2.1** `Module::parse(mdat, smpl) -> Result<Module, ParseError>`: magic, text area,
      96-word table. Borrowed slices, no sample copying. — *check: unit test asserts magic,
      song count, tempo values for a known file* *(Sonnet 5)*
- [x] **2.2** Layout detection (zero check at `$1D0`) and resolution of the trackstep,
      pattern-pointer and macro-pointer tables. — *check: all 10 corpus files parse; detected
      variant matches `testdata/README.md`* *(Sonnet 5)*
- [x] **2.3** Bounds-checked accessors `pattern(n)`, `macro_(n)`, `sample(offset, len)` — the
      trust boundary, never raw indexing. — *check: truncated buffer and corrupted offset table
      both return `Err` without panicking* *(Sonnet 5)*

### Phase 3 — Paula mixer

- [x] **3.1** `Voice { start, len, period, volume, dma_on, loop_start, loop_len }` plus
      fractional position, and the setters the sequencer calls. *(Sonnet 5)*
- [x] **3.2** `Paula::render()`: linear interpolation, one-shot → loop transition, hardware
      panning with the separation knob, volume scaling, clamped output. — *check: synthetic
      1000 Hz sine at a known period, count zero crossings, assert within 0.5 %* *(Sonnet 5)*
- [x] **3.3** DMA state feedback: expose the loop-completion count for macro `1A`. — *check: a
      100-sample loop rendered for 10 loop lengths reports 10* *(Sonnet 5)*

### Phase 4 — Sequencer (the hard part)

- [x] **4.1** Tick scheduling: `samples_until_next_tick`, the 50 Hz path and the CIA path,
      block-size independence. — *check: 1 second rendered as one 48000-frame call and as 480
      hundred-frame calls is bit-identical* *(Opus 5)*
- [x] **4.2** Trackstep runner: `$EFFE` commands, `$80` hold with transpose, `$FF`/`$FE` stop,
      song start/end/loop. — *check: trace the first 200 ticks and verify against
      `docs/format.md`* *(Opus 5)*
> Finding from 4.2: the format gives the trackstep exactly one shared line
> pointer (one `PlaySection`/song-start/song-end index, not one per track),
> so the "8 tracks, 1 line" model is settled. What is *not* settled, because
> it needs a pattern to exist at all: whether the line advance is gated on
> every active track's pattern reaching `$F0 <End>`, or fires unconditionally
> per tick. `Sequencer::advance()` is deliberately an explicit,
> externally-triggered step (not tied to `TickClock`) so step 4.3 can wire in
> whichever trigger the pattern decoder needs without reshaping this step's
> API. Also inferred, absent a stated rule: `PlaySection`'s `times` counts
> *additional* repeats after the one just played (mirrors pattern `$F1
> <Loop>`'s "repeats ... `aa` times"), and reaching past `song_end` with no
> redirecting command falls back to looping to `song_start`.

- [x] **4.3** Pattern decoder: longword classification, notes with detune, notes with wait,
      portamento, `$F0`–`$FF`. Triggers macros with channel, note, volume. — *check: pattern
      dump is self-consistent* *(Opus 5)*

> Finding from 4.3: the Finding from 1.3 holds live — every `$F1`/`$F2`/`$F8` target reachable
> from a song in the corpus lands inside the named pattern's own longword count when read as a
> relative step index. Two corpus quirks a pattern walker must tolerate: a song's last trackstep
> line can already be pattern data (`mdat.r-type` song 0 line 79), so not every "pattern number"
> reachable from the trackstep table points at a pattern; and most patterns end in an infinite
> `$F1`, leaving their `$F0 <End>` unreachable.
- [x] **4.4** Macro interpreter: per-voice PC and wait counters, note table, transpose + detune
      → period, envelope, vibrato, portamento, sample start/length/loop, DMA, one-shot,
      wait-on-DMA. Unknown `$22`–`$29` recorded, never guessed. — *check: 30 s render is not
      silent, no `NaN`/`inf`, no clipping, and bit-identical across two runs* *(Opus 5)*

> Finding from 4.4: resolves the trackstep-advance question left open since 4.2/4.3 (§7 of
> `docs/playback-model.md`) — the shared line pointer advances **unconditionally every jiffy**,
> not gated on any track's pattern reaching `$F0 <End>`. The Finding from 4.3 forced this: most
> corpus patterns end in an infinite `$F1` and never reach `$F0` at all, so an End-gated advance
> would hang forever on real data. `docs/opcodes.md` §1's per-track word table already fits this
> reading — `$80 <Hold>` exists precisely so most jiffies' trackstep evaluation is a no-op that
> only refreshes transpose, while the pattern's own wait/loop opcodes carry the actual rhythm.
> `Player` (new, `docs/architecture.md` §3) owns this loop plus the register-seam wiring: each
> jiffy, trackstep → (re)load per-track `PatternRunner`s → dispatch notes and `$F5`-`$F7`/`$FC`
> effects to the target voice's `MacroInterpreter` → tick all four macro programs → `Paula`
> renders the chunk up to the next tick boundary. Also settled, each flagged in code as this
> crate's own reading where [S1] under-specifies: a pattern note's voice nibble is masked to
> `0`-`3` (Paula has only four hardware voices); `$18 <Sampleloop>`'s running loop-region offset
> is separate from the attack region set by `$02`/`$03`, matching the "not idempotent" gotcha;
> and `$21 <Play macro>` keeps the target voice's current note/volume/transpose rather than
> resetting them, since [S1] gives it no note operand of its own.

### Phase 5 — CLI

- [x] **5.1** `tfmx-cli render <mdat> <smpl> -o out.wav [--song N] [--seconds S] [--rate HZ]
      [--separation P]`. — *check: playable WAV of the requested length* *(Haiku 4.5)*
- [x] **5.2** `tfmx-cli info`: header text, songs, tempos, detected layout, and the
      `unsupported_ops()` histogram. — *check: run across the corpus* *(Haiku 4.5)*

### Phase 6 — Verification and tuning

- [x] **6.1** Golden-hash regression tests: SHA-256 of the first 10 s per module in
      `tests/golden.txt`, with a documented regeneration command. — *check: perturbing the
      volume scale makes it fail* *(Sonnet 5)*

> Finding from 6.1: the golden-hash render surfaced a real bug the 1-second `info` check never
> reached. `mdat.r-type` song 0's declared (inclusive) `song_end` is line 79, which the song
> loops back onto every ~80 ticks; `decode_track_word` read that line's track-0 word (hi byte
> `$FA`) as pattern number 250, out of the 128-pattern range, and `Player::render` propagated
> the resulting `AccessError` as fatal, aborting the whole render partway through. `$00`-`$7F`,
> `$80`, `$FE`, `$FF` are the only hi-byte values `docs/format.md` §5 documents; `$81`-`$FD` is
> unstated. Fixed by masking the pattern-number branch to 7 bits (`number & 0x7F`) instead of
> using the raw byte, matching the existing tolerance for the voice nibble in `Player::voice_of`
> — 128 patterns only ever need 7 bits, so a stray top bit is dropped rather than erroring out
> the render. All 10 corpus files now render 10 s of song 0 without error; `tfmx/src/sequencer.rs`
> carries a unit test built from the real `mdat.r-type` word.
- [x] **6.2** A/B listening pass against a reference recording. Judge in this order: **tempo**
      (the classic bug), pitch, instrument attack, timbre. — *check: per-module notes in
      `docs/status.md`, remaining deviations listed rather than hidden* *(Opus 5)*

> 6.2 could not be literal listening; it used signal-processing proxies against `uade123` instead
> (envelope cross-correlation for tempo, long-term log-spectrum detune for pitch, onset rise time
> for attack, spectral-shape correlation for timbre) — see `docs/status.md` for the full method
> and per-module numbers. Tempo is clean on all 10 corpus modules (drift under one measurement
> frame everywhere). Pitch could not be conclusively verified by signal analysis alone on this
> polyphonic material against a differently-implemented reference; confidence there rests on the
> existing `note_period()` and `Paula` period/frequency unit tests plus the 6.1 golden-hash lock,
> not on this pass. Timbre is broadly similar (8/10 modules ≥0.71 spectral-shape correlation);
> `turrican outside` and `turrican 2 level 3-flight` are lower (0.567, 0.642) and unverified either
> way. `docs/status.md` names an actual human listening pass as the honest next step if either is
> ever suspected of a real bug — not more automated analysis.

---

## M2 — Desktop realtime player

New binary crate `tfmx-play`, terminal-based (plain raw-mode keys, no TUI framework). Reuses
`Player::render()` unchanged, per `docs/architecture.md` §7/§9 — realtime is a new *consumer* of
the core, not a change to it.

### Phase 7 — `tfmx-play` ✅

- [x] **7.1** `tfmx-play <mdat> <smpl> [--song N]`: new binary crate, opens the default `cpal`
      output device, drives `Player::render()` from its audio callback, runs until Ctrl+C. No
      pause/stop/song-switch yet — just sound out. No resampling: `Player` already handles an
      arbitrary sample rate exactly (step 4.1), so it is simply constructed at the output
      device's own reported rate. The one piece worth isolating and testing is the sample-format
      conversion (core `i16` → whatever format `cpal`'s default output config actually is, e.g.
      `f32` on most backends) as a pure function; the `cpal` device/stream plumbing itself is
      thin, untested glue, same boundary `tfmx-cli`'s `main()`/`hound` I/O already draws. —
      *check: unit tests cover CLI arg parsing and the `i16`→device-format conversion function; a
      manual run audibly plays a corpus module through the default output device and exits
      cleanly on Ctrl+C* *(Opus 5)*
- [x] **7.2** Transport controls: raw-mode terminal keys — space = pause/resume, `n`/`p` = next/
      previous song (rebuilds `Player` for the new song from the already-parsed `Module`), `q` =
      quit. Key events reach the audio callback via a channel; paused output is silence, not a
      stopped stream (avoids device reopen latency). — *check: unit tests cover the
      command-channel state machine (pause/resume/song-switch/quit) independent of any real
      device or terminal; a manual run confirms pause is silent and gapless, resume continues
      without a glitch, and song-switch changes what's audible* *(Opus 5)*

> Finding from 7.2: raw mode turns off the terminal's normal LF→CRLF translation, so a plain
> `eprintln!`/`\n` after `crossterm::terminal::enable_raw_mode()` only moves the cursor down, not
> back to column 0 — repeated status prints stair-step further right on every line instead of
> updating in place. Fix: never print a bare `\n` once raw mode is on; use `MoveToColumn(0)` +
> `Clear(ClearType::CurrentLine)` + `Print(..)` (no trailing newline) instead, and print anything
> that *does* want normal line behavior (e.g. the one-time controls hint) before raw mode is
> enabled, not after. A final bare `eprintln!()` right before `disable_raw_mode()` keeps the
> shell's next prompt off the end of the last status line.

M2 is complete: `cpal` output, play/pause/stop, and song selection, exactly the milestone's own
one-line scope — no further phases planned for it right now.

---

## M3 — Web

New crate `tfmx-web`, wasm-bindgen wrapper around the existing `tfmx` core (`docs/architecture.md`
§7/§9 already scopes this: no core change required), driven from an `AudioWorklet`. Plain
HTML/CSS/JS demo page, no bundler — `tfmx-web`'s own `wasm-bindgen --target no-modules` output is
the only build step.

**Design decisions carried into these steps:**
- `AudioWorkletGlobalScope` does not reliably support ES-module `import` across browsers, so the
  wasm glue is built with `wasm-bindgen --target no-modules` (classic-script output) and loaded
  inside the worklet via `importScripts()`. The main thread fetches and compiles the `.wasm` once
  and transfers the resulting `WebAssembly.Module` to the worklet, which instantiates it
  synchronously with `initSync()` (no top-level `await` inside a worklet).
- `#[wasm_bindgen]`-attributed types (`JsValue`/`JsError`) only compile and run under `wasm32`
  with the JS glue present, so they cannot be exercised by plain `cargo test`. `tfmx-web` is
  therefore split into a plain-Rust core (parses `Module`, owns a `Player`, fully host-testable)
  and a thin `#[wasm_bindgen]` shell that only marshals `&[u8]`/`Uint8Array` and maps errors —
  the same "push glue to the boundary, keep the core plain" pattern already used at the CLI and
  `cpal` boundaries.

### Phase 8 — `tfmx-web` core + wasm boundary

- [x] **8.1** Plain-Rust core (no `wasm_bindgen` attributes): parses `mdat`/`smpl` byte slices via
      `Module::parse`, owns a `Player`, exposes `render(&mut self, out: &mut [i16])` and
      `set_song(&mut self, song: u32)` (rebuilds `Player` from the already-parsed `Module`,
      mirroring `tfmx-play`'s 7.2 approach). — *check: `cargo test -p tfmx-web` on the host target
      covers construction from a corpus module, `render()` producing non-silent/non-`NaN` output,
      and `set_song` switching songs* *(Sonnet 5)*
- [x] **8.2** Thin `#[wasm_bindgen]` wrapper: `TfmxWeb::new(mdat: &[u8], smpl: &[u8], sample_rate:
      u32) -> Result<TfmxWeb, JsError>`, `render`, `set_song`, delegating straight to 8.1's core. —
      *check: `wasm-bindgen-test` (Node target) constructs a `TfmxWeb` from a corpus module's
      bytes, renders a block, asserts non-empty/non-panicking output; `cargo build -p tfmx-web
      --target wasm32-unknown-unknown --target-dir target/wasm` (a non-default invocation — not
      part of `cargo build --workspace`) succeeds* *(Sonnet 5)*

### Phase 9 — AudioWorklet integration

- [x] **9.1** Pure JS conversion function: interleaved `i16` (what `render()` produces) → planar
      `f32` per channel (what `AudioWorkletProcessor.process()`'s output array wants) — isolated
      exactly like `tfmx-play`'s 7.1 format-conversion function. — *check: plain `assert`-based
      Node script (`node test.js`, no framework), known input → expected per-channel output*
      *(Sonnet 5)*
- [x] **9.2** `tfmx-processor.js` (`--target no-modules` glue loaded via `importScripts`,
      `initSync` from a transferred `WebAssembly.Module`, calls the 8.2 wrapper's
      `render`/`set_song` from `process()`, applies 9.1's conversion) + main-thread bootstrap
      (fetch+compile the `.wasm` once, `audioContext.audioWorklet.addModule(...)`, post the
      compiled module and file bytes to the worklet node). — *check: manual run — constructing the
      node and starting the context produces audible output from a corpus module* *(Opus 5)*

> Finding from 9.2: the plan's `--target no-modules` + `importScripts` design was wrong.
> AudioWorklet processor modules are always ES modules per the Worklet spec, in every browser --
> there is no `importScripts` in `AudioWorkletGlobalScope`, unlike dedicated/shared workers. Built
> with `wasm-bindgen --target web` instead, loaded via ordinary static `import`. Two further
> platform gaps, both real, not code bugs: `AudioWorkletGlobalScope` has no `TextDecoder` (needed
> by the generated glue for Rust error strings, worked around with a small polyfill), and a
> worklet's incoming message queue is only pumped while its `process()` is running, which itself
> needs a live connection to the destination -- so the main thread must `connect()` the node
> *before* handshaking init data over its port, not after.

### Phase 10 — Demo page

- [x] **10.1** Static HTML/CSS/JS page, no bundler: a drop zone accepting two files, paired by the
      existing `mdat.*`/`smpl.*` filename convention (`testdata/`, `docs/format.md`) — a visible
      error if the pair doesn't resolve unambiguously; play/pause, song-select, wired to the
      worklet node's message port. — *check: manual run in a real browser — drag a corpus
      mdat/smpl pair in, hear it play; pause/resume is silent and gapless; song-switch changes
      what's audible (same bar as 7.2)* *(Sonnet 5)*

M3 is complete when Phase 10 is checked off. Stop there for explicit approval before any "Beyond"
work.

---

## M4 — Diagnostics tooling

The player is unobservable: `Player`'s only public getter is `unsupported_ops()`. Nothing outside
the crate can see the current trackstep line, which pattern each track runs, per-voice macro state,
or Paula's register values. That is why `docs/status.md`'s open follow-up is stuck — its own named
next step is a "trackstep/pattern trace diagnostic" that exists only as throwaway code from a past
conversation, and step 6.2's automated proxies measured pitch/timbre correlation rather than the
question actually being asked, which is *does this sound like a coherent song at all*.

This milestone builds the observation seam and the three tools that turn it into answers, ordered
so the fastest route to **hearing** the problem lands first. It delivers tools and a corpus-wide
diagnosis, **not a fix** — what to repair is a separate call at the phase gate.

**Design decisions carried into these steps:**
- **The trace seam is generic, not dynamic.** `Player::render` gains a private generic sibling
  `render_inner<F: FnMut(TraceEvent)>(&mut self, out, trace: F)`; `render()` calls it with `|_| {}`,
  which monomorphizes to exactly today's code. That is what keeps the step 6.1 golden hashes
  byte-identical and proves the seam is inert. This mirrors the crate's existing idiom —
  `PatternRunner::advance` and `MacroInterpreter::tick` already take `emit: impl FnMut(..)`.
- **Mute happens at the mix, not the fetch.** In `Paula::render`, `voice.next_sample(..)` must still
  run for a muted voice so playback position, loop completions and the attack→loop handoff evolve
  identically; only the accumulated amplitude is dropped. This is what makes the four stems sum back
  to the full mix — a testable property, and the reason stems are a trustworthy diagnostic rather
  than a re-simulation.
- **Stems are four renders, not four output buffers.** `render()` is deterministic, so soloing each
  voice in turn costs ~20 lines of CLI code against reshaping `Paula::render`. 4× render time is
  irrelevant offline. (`ponytail:` 4× cost; single-pass Paula output if it ever matters.)
- **Trace output is text; other formats are a match arm away.** Format selection is a plain
  `--format` enum over one pure function per format (`fn write_text(e: &TraceEvent, out: &mut impl
  Write)`), not a trait. Adding JSON or TOON later is a new function plus one arm.
- **Core stays core.** `TraceEvent` is a plain enum in `tfmx` — no dependency, no allocation, no I/O,
  still builds for `wasm32`. All formatting, folding and analysis lives in `tfmx-cli`, the same
  boundary already drawn at the CLI, `cpal` and wasm edges.

### Phase 11 — Observation seam and diagnostic tools

- [x] **11.1** Voice mute in `Paula`: `muted: [bool; 4]` plus `set_voice_muted(voice, bool)`, and a
      forwarding `Player::set_voice_muted`. Copy `self.muted` into a local before the `iter_mut()`
      loop (borrow), call `next_sample` unconditionally, skip only the `+=` into the pan
      accumulators. Also promote `Paula::voice(&self, voice) -> Voice` from `#[cfg(test)]
      pub(crate)` to a public getter — 11.3 needs it and `Voice`'s fields are already `pub`. —
      *check: unit tests that (a) muting makes a single-voice render silent, (b) a muted voice's
      playback position still advances, (c) all four solo renders sum sample-for-sample to the full
      mix on non-clipping material; golden hashes unchanged* *(Sonnet 5)*
- [x] **11.2** `tfmx-cli render` gains `--solo N`, `--mute N[,N..]` and `--stems`; `--stems` derives
      four filenames from `-o` (`out.wav` → `out-v0.wav` … `out-v3.wav`) and runs four soloed
      renders. — *check: unit tests on the filename derivation and on mute/solo → mask resolution as
      pure functions; a manual run produces four WAVs whose sum matches the single-file render.*
      **Then actually listen**: per-voice stems of `apidya (title)` and the suspect `turrican`
      module, with what was heard recorded in `docs/status.md`. *(Sonnet 5)*
- [x] **11.3** `TraceEvent` (new `tfmx/src/trace.rs`, re-exported from `lib.rs`) and
      `Player::render_traced`. Variants: `Jiffy { frame, line, tempo, stopped }`,
      `Trackstep(TrackstepLine)`, `Pattern { track, pattern, step, entry }`,
      `Trigger { voice, macro_number, note, volume, transpose }`, `Voice { voice, state }`.
      `Trigger` is emitted from `dispatch_pattern_entry` rather than derived by the consumer,
      precisely because `voice_of()`'s nibble masking is a documented uncertainty — a masking bug
      has to be visible. Thread `trace: &mut impl FnMut(TraceEvent)` through `run_jiffy` and
      `dispatch_pattern_entry`. `Voice` events fire for all four voices at each jiffy's end;
      de-duplication is the consumer's job, not the core's. Record the seam in
      `docs/architecture.md` alongside the register seam. — *check: (a) `render()` and
      `render_traced(.., |_| {})` are bit-identical on a corpus module; (b) the existing golden
      hashes are unchanged — the load-bearing proof the seam is inert; (c) a 1 s traced render emits
      ~50 `Jiffy` events with monotonically non-decreasing `frame`* *(Opus 5)*
- [x] **11.4** `tfmx-cli trace <mdat> <smpl> [--song N] [--seconds S] [--voice N] [--track N]
      [--format text]`, writing to an injected `&mut impl Write` following `run_info`'s existing
      testable shape. Suppresses unchanged `Voice` events. Aligned, greppable, `diff`-able between
      two runs. — *check: unit tests over a hand-built `Vec<TraceEvent>` → expected text, with no
      corpus and no player involved; a corpus run of two songs produces visibly different traces*
      *(Sonnet 5)*
- [x] **11.5** `tfmx-cli lint`, and `info` made static. `info` drops its hidden silent render and
      its histogram (its test updates with it); everything requiring playback moves to `lint`.
      `lint <mdat> <smpl> [--song N] [--seconds S]` folds a traced render plus PCM statistics into a
      report, as a **pure function over `&[TraceEvent]`** so every finding is testable from a
      synthetic event vector. *Summary:* jiffies run, tempos seen, trackstep lines visited, looped
      or stopped, distinct patterns per track, distinct macros and note-on count per voice,
      pattern-command histogram, unsupported-opcode histogram. *Findings:* `dead-voice` (DMA never
      on), `frozen-voice` (period/volume/region unchanged >2 s with DMA on), `no-retrigger` (sample
      region never changes for the whole run — **the `apidya (title)` symptom as described**),
      `single-pattern`, `stopped-early`, `silence`, `clipping`, `unsupported-ops`. — *check: unit
      tests drive each finding from a synthetic event vector; then run `lint` across all 10 corpus
      modules and record the table in `docs/status.md` — that corpus-wide result is the actual
      deliverable* *(Opus 5)*
- [x] **11.6** Mutation robustness test (`tfmx/tests/`): a seeded LCG (no dependency) flips bytes in
      corpus `mdat`/`smpl` buffers, then asserts `Module::parse` followed by ~1 s of
      `Player::render` never panics — `Err` is a fine outcome, a panic is not. The stable-toolchain
      version of `cargo-fuzz`, targeting a class of bug that has already bitten twice: step 6.1's
      out-of-range pattern number and step 10.1's `song_start` past the trackstep table, the latter
      fatal in the browser because a panic in `AudioWorkletProcessor.process()` silences the node
      permanently. — *check: passes over a few thousand mutations of all 10 corpus files;
      deliberately reverting the `number & 0x7F` mask in `sequencer.rs` makes it fail* *(Sonnet 5)*

Phase 11's gate: whatever the stems and `lint` turn up is written into `docs/status.md`. Deciding
what to repair — and whether the next round is that repair or the export tooling below — is a
separate call made there. **Human listening is required before this phase is called done**, per the
project's standing rule; no correctness claim rests on automated analysis alone.

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
