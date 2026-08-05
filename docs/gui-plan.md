# Local GUI for TFMX tooling — architecture plan

**Status**: architecture approved by the user (2026-08-05). Crate name chosen:
`tfmx-web-gui` (renamed from an initial `tfmx-gui` draft, per explicit user request).
**Phase G0 done (2026-08-05)**: golden-hash tests for `run_render_macro`'s and
`run_render_pattern`'s WAV output added to `tfmx-cli/src/main.rs`'s test module
(`render_macro_output_matches_golden_hash`, `render_pattern_output_matches_golden_hash`,
SHA-256 over the decoded i16 samples, mirroring `tests/golden.rs`'s own hashing
convention). Both proven to fail on a one-sample perturbation (temporarily reverted,
`git diff` clean). The two existing disasm tests already pin text byte-for-byte, per
this phase's own note — no new disasm test needed. Full workspace suite green (151
`tfmx` + 102 `tfmx-cli` + 31 `tfmx-analysis` tests), clippy clean (same one
pre-existing unrelated `mutation_robustness.rs` warning as every prior phase).
**Phase G1 done (2026-08-05)**: new `tfmx-analysis/src/disasm.rs` — `DisasmLine`
(`Macro { step, opcode, aa, bb, cc }` / `Pattern { step, entry: tfmx::PatternEntry }`),
`disassemble_macro`/`disassemble_pattern`, ported straight from `run_disasm`'s two
match arms (same `MAX_DISASM_STEPS`/terminator-stop logic, moved out of `main.rs`).
Not `serde`-gated: `Pattern` embeds `tfmx::PatternEntry` directly rather than a
mirrored view type, since nothing consumes disasm as JSON yet — add a mirror if that
changes. `tfmx-cli`'s `run_disasm` now just calls the two functions and a new
`format_disasm_line` renders each line back to the exact pre-extraction text (opcode
name lookup stays in `tfmx-cli`, a display concern). The two former corpus-based
disasm tests moved into `tfmx-analysis/src/disasm.rs` asserting on `DisasmLine`
values; `tfmx-cli` keeps its usage-validation test plus one new thin formatting-only
test pinning exact text for one macro/pattern line, no corpus needed. Full workspace
suite green, `cargo fmt --check` clean, clippy clean (same one pre-existing warning),
`wasm32-unknown-unknown` unaffected, golden hashes byte-identical.
**Phase G2 done (2026-08-05)**: new `tfmx-analysis/src/render.rs` —
`render_macro_pcm(module, macro_number, note, volume, voice, tempo, rate, separation,
total_frames) -> Result<Vec<i16>, AccessError>`, ported from `run_render_macro`'s
tick-then-render loop. Collapsed to one allocation and one `TickClock::advance` call
over the full duration: the original's 4096-frame chunking existed only to bound the
buffer handed to the streaming `hound` writer, which doesn't exist on this side of the
extraction, so nothing forces re-chunking. `tfmx-cli`'s `run_render_macro` now just
calls it and `hound`-writes the returned buffer. New test
`render_macro_pcm_matches_g0_golden_hash` hashes the raw `Vec<i16>` against G0's
existing WAV-bytes golden hash directly (byte-identical, since 16-bit PCM WAV is a
lossless container) — confirms the extraction changed nothing. Full workspace suite
green (151 `tfmx` + 101 `tfmx-cli` + 34 `tfmx-analysis` tests), `cargo fmt --check` and
clippy clean on the touched crates (no new warnings).
**Phase G3 done (2026-08-05)**: `tfmx-analysis/src/render.rs` gains
`render_pattern_pcm(module, pattern, transpose, tempo, rate, separation,
total_frames) -> Result<Vec<i16>, AccessError>` and its private helper
`dispatch_pattern_entry_standalone`, ported from `run_render_pattern`'s
`PatternRunner` + 4-voice `MacroInterpreter` + `Paula` loop, preserving the
PPat/track-operand simplification and jump/lock/multi-macro state exactly as
commented at the original call site. `tfmx-cli`'s `run_render_pattern` now
just calls it and `hound`-writes the returned buffer. Added the WAV-length
test render-pattern was missing
(`render_pattern_writes_a_wav_of_the_requested_length`, mirroring
render-macro's); G0's `render_pattern_output_matches_golden_hash` still
passes unmodified. Full workspace suite green (151 `tfmx` + 102 `tfmx-cli` +
31 `tfmx-analysis` tests), clippy clean on the touched crates (same one
pre-existing unrelated `mutation_robustness.rs` warning as every prior
phase), `wasm32-unknown-unknown` build for `tfmx` unaffected.
`cargo fmt --check` reports diffs across the repo, including files this
phase never touched -- confirmed pre-existing (stashed this phase's changes,
diffs remained) and due to local `rustfmt` 1.9.0 disagreeing with whatever
version originally formatted the repo (no `rustfmt.toml` pins one); not a
G3 regression.
**Phase G4 done (2026-08-05)**: workspace verification only — G1-G3 had already
thinned `run_render_macro`/`run_render_pattern`/`run_disasm` down to arg-parsing +
calling the extracted `tfmx-analysis` functions + writing output, so there was no
leftover duplicated logic in `tfmx-cli/src/main.rs` to remove (checked: no orphaned
`PatternRunner`/`MacroInterpreter`/`dispatch_pattern_entry` internals remained
there). Full `cargo test --workspace` green (151 `tfmx` + 34 `tfmx-analysis` + 102
`tfmx-cli` + `tfmx-play`/`tfmx-web` suites, G0's golden-hash tests included,
bit-identical), `cargo clippy --workspace --all-targets` clean (same one
pre-existing unrelated `mutation_robustness.rs` warning as every prior phase). The
"Files to touch" checklist below is fully satisfied for the `tfmx-analysis`/
`tfmx-cli` extraction (`tfmx-web-gui` itself is Phase W0, not yet started).
**Phase W0 done (2026-08-05)**: new `tfmx-web-gui` binary crate, added to workspace
`members`, depending on `tfmx`, `tfmx-analysis` (`serde` feature) and `tiny_http`
(0.12). `src/main.rs` is a minimal `tiny_http` server serving `static/index.html`
(a placeholder page) plus static assets under `static/`, with a path-traversal
guard (`src/static_files.rs`'s `resolve`, canonicalizes and checks
`starts_with(static_dir)`) and a small extension-to-MIME-type map. `src/session.rs`
adds `Session`: owns the loaded `mdat`/`smpl` bytes and hands out a freshly
`Module::parse`d `tfmx::Module` per call rather than caching one — every
`tfmx-analysis` entry point (`render_*_pcm`, `disassemble_*`, `build_song_view`)
already takes `&Module` and builds its own transient state per call (confirmed by
reading `render_macro_pcm`'s signature), and this GUI's scope is render-ahead-to-
WAV-blob, not live streaming, so there is no persistent `Player` to keep between
requests — unlike `tfmx-web`'s `Core`, which leaks `mdat`/`smpl`/`Module` to
`'static` because a wasm page's `Core` lives exactly as long as the tab; that
pattern doesn't fit a long-running server process reloading modules repeatedly via
`POST /load`, so `Session` re-parses instead (cheap: `Module::parse` only reads the
fixed header). `Session` validates the pair parses at `load()` time so a bad file
errors immediately rather than on first later request. `Session` isn't wired into
a route yet (that's Phase W1), so `mod session;` in `main.rs` carries a `ponytail:`
`#[allow(dead_code)]`. TDD'd: 8 new tests (`session`: loads a valid corpus module
and re-parses consistently, rejects a missing file, rejects a non-module file;
`static_files`: root resolves to index.html, query strings stripped, traversal
rejected, missing file rejected, content-type lookup). Manually smoke-tested with
`curl`: `GET /` → 200 with the placeholder HTML, `GET /../Cargo.toml` → 404
(traversal blocked), `GET /nope` → 404. Full `cargo test --workspace` green,
`cargo fmt --check` and `cargo clippy --workspace --all-targets` clean (same one
pre-existing unrelated `mutation_robustness.rs` warning as every prior phase).
**Next: Phase W1** (routes over `Session`).

## Context

Right now every capability (render, disassemble, render a single pattern/macro,
inspect a song, visualize waveform/call-graph/trackstep) is reachable only via
`tfmx-cli` subcommands with hand-typed file paths. The goal is a GUI where song
files can be picked and these views browsed interactively, without inventing new
analysis logic — everything needed already exists in `tfmx-analysis` (the
`SongView` view-model, explicitly documented as "pure data... so any consumer —
the tfmx-cli renderer, **a future GUI**, another export — can read the same
JSON") and in `tfmx-cli`'s command handlers (disasm, dump, render-pattern,
render-macro, trace, visualize).

Chosen direction: a small local Rust server + a plain HTML/JS browser page,
reusing the no-build-step style already established in `tfmx-web/demo`. The
open question this plan answers is how to shape that server so a **future
native desktop app** (egui, or anything else) is an additive sibling later, not
a rewrite.

## Survey of what already exists (from the exploration that grounded this plan)

- **`tfmx-cli` subcommands** (`tfmx-cli/src/main.rs`): `render`, `info`, `trace`
  (text/json), `lint`, `disasm` (`--macro`/`--pattern`), `onset-diff`,
  `render-macro`, `render-pattern`, `measure-pitch`, `dump` (json), `export-midi`,
  `fidelity-scoreboard`, `export-instruments` (wav/sfz/dspreset), `visualize`
  (self-contained HTML: inline SVG waveform + Mermaid call graph with
  Diagram/Source tabs + trackstep table).
- **`tfmx-analysis`** (`tfmx-analysis/src/{lib,walker,zones,view}.rs`): `SongView`
  via `build_song_view(module, song)` — `WaveformView`, `WalkResult` (reachable
  patterns/macros + dedup'd call-graph edges), `TrackstepMap`. Optional `serde`
  feature. Pure data, no HTML — documented as being for exactly this kind of
  future GUI consumer.
- **`tfmx-web`**: already has a working browser demo (wasm-bindgen + AudioWorklet):
  drag/drop `mdat`/`smpl` pairs, Play/Pause button, song-select `<select>`. Served
  via plain `python3 -m http.server`, no build tooling. `tfmx-web/js/tfmx-bootstrap.js`
  + `tfmx-processor.js` drive playback; `tfmx-web/src/lib.rs`'s `Core` (`new`,
  `render`, `set_song`) is the wasm-bindgen-wrapped playback engine.
- **`tfmx-play`**: native terminal player, `cpal` backend, `Transport` state
  machine (play/pause/next/prev song) driven by `crossterm` keyboard input via
  an `mpsc` channel into the `cpal` callback. No seek/scrub. This is the pattern
  a future native GUI's live playback would reuse.
- **Core `tfmx` crate**: `Player::new(module, song, sample_rate, separation)`,
  `render(out: &mut [i16])` / `render_traced(out, |TraceEvent| ..)`. No seek API —
  song switching means constructing a new `Player`. `Module::parse(mdat, smpl)`
  borrows caller-owned buffers (no lifetime-free owned variant — deliberate, per
  `docs/architecture.md`).
- **No existing GUI framework dependency** anywhere in the workspace (no egui/
  iced/tauri/dioxus/slint/gtk). `proxy.mjs` and `x-out.html` at the repo root are
  unrelated (a Claude-Code request-log proxy, and a saved sample `visualize`
  output) — not prior UI scaffolding, despite living at the root.

## The one architectural principle this plan hinges on

**Keep a transport-agnostic "core" layer and make the HTTP server a thin,
disposable adapter over it.** Concretely:

1. **Functions return structured data, not files or strings.** Today
   `render-pattern`/`render-macro` build a `Player`, call `render()` into a
   buffer, and hand it to `hound` to write a `.wav`. Extract the "build the
   synthetic pattern/macro `Player` and fill a PCM buffer" part into a plain
   function returning `Vec<i16>` (or writing into a caller-supplied buffer,
   mirroring `Player::render`'s own signature). `hound`-writing stays a
   one-line adapter in `tfmx-cli`; a new "wrap PCM as an in-memory WAV byte
   vec for an HTTP response" is an equally thin adapter in the server; a
   future native app skips both adapters and pumps the same buffer into a
   `cpal` callback (as `tfmx-play` already does).
2. **`disasm`'s listing becomes a structured `Vec<DisasmLine>`** (in
   `tfmx-analysis`, alongside `SongView`) instead of being built as
   print-formatted text inline in `tfmx-cli/src/main.rs`'s `Disasm` handler.
   The CLI formats it to a terminal listing; the server serializes it to JSON
   for the page to render as a `<pre>`/table; a native app would iterate it
   directly into widgets. `dump` needs no such extraction — it already
   returns `SongView` (serde-ready).
3. **No HTML/SVG generation logic moves into `tfmx-analysis`.** `visualize.rs`
   already proves the pattern: it's a *consumer* of `SongView`, not part of
   it. The new server's HTML/JS is the same kind of consumer, kept in the new
   crate, not leaking into analysis code a native app would also depend on.
4. **Local server reads files by path, not by browser upload.** Since the
   server runs on the same machine as the files, give it a "list `.mdat`/
   `.smpl` pairs under a directory" endpoint and load by path server-side,
   rather than routing bytes through an `<input type=file>` upload. This
   keeps "how a file gets selected" (browse a local directory, pick a pair)
   architecturally identical to what a native file-open dialog would do later
   — the browser upload path would be a dead end to unwind.
5. **A session/state struct, not one-shot-per-request reloading.** Wrap
   `Module` + current `Player`/song in a small `struct Session { .. }` owned
   by the server (single global instance is fine — this is a local, one-user
   tool, no auth/multi-tenant concerns). This is exactly the shape a native
   app's `App` state struct would hold directly; naming and structuring it
   now avoids redesigning state handling when a native UI is added.

None of this is speculative scaffolding for the native app itself — no egui
code, no trait for "a UI backend," nothing installed for it. It's just picking
function signatures now (return data, not files/strings) that don't have to
change later.

## Crate layout

New crate: **`tfmx-web-gui`** (binary), sibling to `tfmx-cli`/`tfmx-play`/`tfmx-web`.
A new crate rather than a `tfmx-cli serve` subcommand because it's a distinctly
launched thing (`tfmx-web-gui` opens a browser tab) and gives a future native
build an obvious sibling name (e.g. `tfmx-web-gui` today, a `tfmx-native-gui`
or similar later — same core dependency, different adapter).

- Depends on: `tfmx`, `tfmx-analysis` (`serde` feature), `tiny_http` (single
  small dependency for HTTP — see Tech stack below), `serde_json`.
- Extracted core pieces this crate and `tfmx-cli` both call (land in
  `tfmx-analysis`, next to `SongView`/`walker`/`zones`, since that's where the
  "pure data over a `Module`" functions already live):
  - `render_macro_pcm(module, macro_number, note, volume, voice, tempo, seconds, rate, separation) -> Vec<i16>`
  - `render_pattern_pcm(module, pattern_number, transpose, tempo, seconds, rate, separation) -> Vec<i16>`
  - `disassemble_macro(module, n) -> Vec<DisasmLine>` / `disassemble_pattern(module, n) -> Vec<DisasmLine>`
  - `SongView`/`build_song_view` (already exists, reused as-is)
- `tfmx-cli`'s `RenderMacro`/`RenderPattern`/`Disasm` handlers get thinned to
  call these and do only arg-parsing + file writing, removing duplicate logic
  rather than adding it — net code goes down.

## Tech stack

- **Server**: `tiny_http` (blocking, single dependency, no async runtime) over
  a handful of routes. A local single-user tool has no concurrency pressure
  that would justify `axum`/tokio; blocking I/O per request is simplest and
  matches the project's existing bias toward minimal dependencies (`tfmx`
  itself has zero, `tfmx-cli`/`tfmx-play` add only what each command needs).
- **Routes** (all under one `Session`):
  - `GET /files?dir=...` — list `.mdat`/`.smpl` pairs found under a directory
    (default: cwd or a configurable root).
  - `POST /load` — `{mdat_path, smpl_path}` → parses `Module`, resets
    `Session`.
  - `GET /song-view?song=N` — `SongView` as JSON (waveform, call graph,
    trackstep — same data `visualize` already builds).
  - `GET /disasm?macro=N` / `?pattern=N` — `Vec<DisasmLine>` as JSON.
  - `GET /render-macro?...` / `/render-pattern?...` — WAV bytes
    (`audio/wav`), built by wrapping the extracted PCM functions with a
    `hound`-equivalent in-memory writer (or reuse `hound` against a
    `Cursor<Vec<u8>>`, which it already supports).
  - `GET /` and static assets — the page itself.
- **Frontend**: plain HTML + vanilla JS, no framework, no build step — same
  style as `tfmx-web/demo`. A file list / song picker, a pattern/macro
  picker feeding an `<audio src="/render-...">`, a disasm `<pre>` panel, and
  the waveform/call-graph panel reusing `visualize.rs`'s existing SVG/Mermaid
  approach (either embed that HTML fragment via a `/song-view.html` route
  that calls straight into `visualize.rs`'s renderer, or read `SongView` JSON
  client-side — start with the former since it's a direct reuse of working
  code, revisit only if the page needs to react to the data instead of just
  displaying it).

## What this deliberately does not do now

- No real-time/streaming playback in the browser (no WebSocket, no
  AudioWorklet reuse from `tfmx-web`) — render-ahead-to-WAV-blob is enough for
  browsing patterns/macros, and matches every render-* CLI command's own
  batch model. Real-time transport control is what the native app would add
  by calling the same core functions from a `cpal` callback instead, per
  `tfmx-play`'s existing `Transport`.
- No new GUI framework, no auth, no multi-session support — single local
  user, single loaded module at a time.

## Files to touch

- New: `tfmx-web-gui/Cargo.toml`, `tfmx-web-gui/src/main.rs` (static-file
  serving; route handlers are W1), `tfmx-web-gui/src/session.rs` (`Session`),
  `tfmx-web-gui/src/static_files.rs`, `tfmx-web-gui/static/index.html`
  (placeholder; the real picker/panel layout is W2). **(crate skeleton done,
  Phase W0; `app.js` and the real page are Phase W2)**
- `tfmx-analysis/src/`: new `render.rs` (the two PCM functions) and `disasm.rs`
  (structured listing), wired into `lib.rs` next to the existing `view.rs`/
  `walker.rs`/`zones.rs` exports. **(done, G1-G3)**
- `tfmx-cli/src/main.rs`: thin the `RenderMacro`/`RenderPattern`/`Disasm`
  handlers down to arg-parsing + calling the extracted functions + writing
  output (WAV/stdout) — remove the now-duplicated logic. **(done, G1-G3,
  confirmed no leftover duplication in G4)**
- Root `Cargo.toml`: add `tfmx-web-gui` to workspace members. **(done, W0)**

## Verification

- `cargo test` across the workspace still passes after the `tfmx-analysis`/
  `tfmx-cli` extraction (existing render-pattern/render-macro/disasm tests
  move with the logic; a golden-output check — same WAV bytes / same disasm
  text before and after the extraction — confirms no behavior change).
- Run `tfmx-web-gui` against a real corpus module from `testdata/`, in a
  browser: list files, load a pair, switch songs, play a rendered pattern and
  a rendered macro, view disasm for a macro and a pattern, view the
  waveform/call-graph/trackstep panel — confirm each matches the equivalent
  `tfmx-cli` command's output for the same module/args.

## Suggested first implementation step

Start with the `tfmx-analysis` extraction (item 1 in "Files to touch") alone,
TDD'd, with a golden-output test proving `render-macro`/`render-pattern`/`disasm`
produce byte-identical output before and after the move — this is a pure
refactor with no new capability, and de-risks the rest before any HTTP code
exists. Only then scaffold `tfmx-web-gui` itself.

## Delegation and model tiers

Per `CLAUDE.md`: hand an agent **only what its subtask needs** — its own row below, the
files/line ranges it names, the `docs/` pages it builds on, the hard rules, and its
`check:`. Not this whole plan.

| Tier | Use for |
|---|---|
| **Haiku 4.5** | Mechanical, fully specified, single-file, criterion is a passing test |
| **Sonnet 5** | Normal implementation with local design judgment |
| **Opus 5** | Ambiguous, cross-cutting, reverse-engineering, or root-cause work |

Nothing here is ambiguous or reverse-engineering work — the architecture is decided and
the code being moved already works — so no subtask needs Opus 5.

### Phase G0 — Golden-output safety net (write before touching any extraction code)

Deliverable: a test pinning today's exact output of `run_render_macro`/`run_render_pattern`/
`run_disasm` for a known corpus module, so every later phase has something to fail against.

| Subtask | Model |
|---|---|
| Extend `tfmx-cli/tests/golden.rs` (or a sibling file reusing its corpus-path/hash helpers) with a SHA-256 pin of `run_render_macro`'s and `run_render_pattern`'s WAV bytes for one known corpus module | Haiku 4.5 |
| Confirm the existing `disasm_macro_lists_a_splitkey_cont_chain_and_stops_at_stop` / `disasm_pattern_matches_the_known_decode_of_pattern_84_step_0` tests in `tfmx-cli/src/main.rs` already pin disasm text byte-for-byte — reuse as-is, no new test needed | Haiku 4.5 |

**check:** new test(s) pass now, unmodified, and are proven to fail on a one-byte change (temporarily flip a sample, confirm failure, revert).

### Phase G1 — Extract disasm into structured data

Deliverable: `tfmx-analysis/src/disasm.rs` with a `DisasmLine` type and
`disassemble_macro`/`disassemble_pattern`; `tfmx-cli`'s `run_disasm` (`main.rs:846-884`)
thinned to format+print.

| Subtask | Model |
|---|---|
| Design `DisasmLine`'s shape (macro-step vs. pattern-step variants, fields, `#[cfg_attr(feature = "serde", ...)]` per `tfmx-analysis`'s existing per-type gating convention) | Sonnet 5 |
| Port the two match-arm loops from `run_disasm` into `disassemble_macro`/`disassemble_pattern`, pushing `DisasmLine`s instead of `writeln!`ing | Haiku 4.5 |
| Thin `run_disasm` to call the new functions and format each line back to today's exact text | Haiku 4.5 |
| Move the two existing disasm tests to assert on structured `DisasmLine`s in `tfmx-analysis`; add a thin formatting-only test in `tfmx-cli` | Haiku 4.5 |

**check:** G0's disasm golden text unchanged; `cargo test -p tfmx-analysis -p tfmx-cli`.

### Phase G2 — Extract `render_macro_pcm`

Deliverable: `tfmx-analysis/src/render.rs` gains `render_macro_pcm(..) -> Vec<i16>`;
`run_render_macro` (`main.rs:537-587`) thinned to call it + `hound`-write the result.

| Subtask | Model |
|---|---|
| Decide the function signature, and whether the chunked-render loop (today streaming straight to `hound` per 4096-frame chunk) can collapse to one allocation now that the output is an owned `Vec<i16>`, or must keep chunking | Sonnet 5 |
| Port the loop into `render_macro_pcm`, preserving the exact tick-then-render order per chunk | Haiku 4.5 (protected by G0's WAV golden test) |
| Thin `run_render_macro` to call `render_macro_pcm` + `hound`-write the buffer | Haiku 4.5 |

**check:** G0's render-macro golden WAV bytes unchanged.

### Phase G3 — Extract `render_pattern_pcm`

Deliverable: `dispatch_pattern_entry_standalone` + the render loop (`main.rs:596-741`) move
into `tfmx-analysis/src/render.rs` as `render_pattern_pcm(..) -> Vec<i16>`;
`run_render_pattern` thinned to match.

| Subtask | Model |
|---|---|
| Decide the signature and move the function + its helper, preserving the already-documented PPat/track-operand simplification and the jump/lock/multi-macro state exactly as commented today | Sonnet 5 |
| Thin `run_render_pattern` to call `render_pattern_pcm` + `hound`-write | Haiku 4.5 |
| Add the WAV-length unit test render-pattern is currently missing, mirroring `render_macro_writes_a_wav_of_the_requested_length` | Haiku 4.5 |

**check:** G0's render-pattern golden WAV bytes unchanged; new length test passes.

### Phase G4 — Workspace verification and cleanup

| Subtask | Model |
|---|---|
| Run `cargo test` across the workspace; confirm all G0 golden checks are bit-identical pre/post extraction | Haiku 4.5 |
| Remove logic now duplicated in `tfmx-cli/src/main.rs`; judgment call on which old inline tests move vs. stay as thin adapter tests, to avoid duplicate coverage | Sonnet 5 |
| Update this file's "Files to touch" checklist and `ROADMAP.md`'s Status block: mark the extraction done, name `tfmx-web-gui` scaffolding (W0) as next | Haiku 4.5 |

**check:** full `cargo test` green; Status block updated; one commit per phase, per `CLAUDE.md`'s turnaround loop.

### Phase W0 — `tfmx-web-gui` crate skeleton

| Subtask | Model |
|---|---|
| `tfmx-web-gui/Cargo.toml`, add to workspace `members`, pick `tiny_http` version, minimal `main.rs` serving a static page | Sonnet 5 |
| `Session` struct design (owns `Module` + current `Player`/song state, per principle 5 above) | Sonnet 5 |

### Phase W1 — Routes (mechanical once G1–G3 exist and `Session` is fixed)

| Subtask | Model |
|---|---|
| `GET /files?dir=` — list `.mdat`/`.smpl` pairs under a directory | Haiku 4.5 |
| `POST /load` — parse + reset `Session` | Sonnet 5 (touches session state/error handling) |
| `GET /song-view?song=` → `SongView` JSON via existing `build_song_view` | Haiku 4.5 |
| `GET /disasm?macro=`/`?pattern=` → JSON via G1's functions | Haiku 4.5 |
| `GET /render-macro`/`/render-pattern` → WAV bytes via G2/G3's PCM functions + in-memory `hound` write | Haiku 4.5 |

### Phase W2 — Frontend

| Subtask | Model |
|---|---|
| Page layout: file/song/pattern/macro pickers, `<audio>` element, disasm panel, waveform/call-graph panel (reuse `visualize.rs`'s HTML fragment) | Sonnet 5 |
| Per-route `fetch` wiring, once route contracts are fixed by W1 | Haiku 4.5 |

**check (W0–W2):** manual browser walkthrough against a real corpus module, matching each `tfmx-cli` command's output for the same args — see Verification above.
