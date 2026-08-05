# Local GUI for TFMX tooling — architecture plan

**Status**: architecture approved by the user (2026-08-05), no implementation started
yet. Crate name chosen: `tfmx-web-gui` (renamed from an initial `tfmx-gui` draft, per
explicit user request).

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

- New: `tfmx-web-gui/Cargo.toml`, `tfmx-web-gui/src/main.rs` (routing),
  `tfmx-web-gui/static/index.html` + `app.js`.
- `tfmx-analysis/src/`: new `render.rs` (the two PCM functions) and `disasm.rs`
  (structured listing), wired into `lib.rs` next to the existing `view.rs`/
  `walker.rs`/`zones.rs` exports.
- `tfmx-cli/src/main.rs`: thin the `RenderMacro`/`RenderPattern`/`Disasm`
  handlers down to arg-parsing + calling the extracted functions + writing
  output (WAV/stdout) — remove the now-duplicated logic.
- Root `Cargo.toml`: add `tfmx-web-gui` to workspace members.

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
