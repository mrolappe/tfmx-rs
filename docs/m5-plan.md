# Milestone M5 — Export and static analysis

> **Approved by the user 2026-08-02. Not started.** The phase list with checkboxes lives in
> [ROADMAP.md](../ROADMAP.md); this file is the full brief behind it. Next up: **Phase 5.0**.

## Context

M1–M4 are done. The macro-playback-fidelity thread is **paused, not closed**: its recurring
failure mode is that structurally correct, TDD'd fixes produce *no audible change*, because
the project has no cheap oracle and every verification round costs a user listening session.
The user chose a direction change — build the "Export and static analysis" milestone: a
**static** module walker that resolves a song to its reachable patterns, macros and sample
regions, with export, verification and visualization on top.

Two facts found while planning reshape it:

1. **A reference Paula register log now looks obtainable.** WinUAE 6.1.0 (Stefan Reinauer's
   cross-platform fork) is installed at `/Applications/WinUAE.app`, has a debugger with
   `Memwatch` breakpoints, and carries symbolic `AUD0LCH/LCL/LEN/PER/VOL`. Paula's audio
   registers are memory-mapped at `$DFF0A0–$DFF0DF`, so a write-watch over that range *is*
   the reference log — captured from **the TFMX editor the user already trusts**, with no
   GPL replayer in the loop. Open risk is narrow: can memwatch *log* rather than *break*,
   and can output be captured to a file in bulk. Hence a timeboxed spike, decided first.
2. **Macros are key/velocity zone maps.** `docs/opcodes.md:176-177` — `$1C <Splitkey>`
   branches on the current note, `$1D <Splitvol>` on volume. Statically resolving those
   branches yields, per macro, a table of `(note range, velocity range) → sample region`.
   That table maps **one-to-one onto SFZ's `lokey`/`hikey`/`lovel`/`hivel`**, and is the
   single intermediate representation feeding MIDI mapping, SFZ/sampler export, sample
   export and tracker export. It is the spine of this milestone.

## Decisions taken (2026-08-02)

| Decision | Choice | Rationale |
|---|---|---|
| Oracle | **Timebox a spike**, then re-decide | WinUAE fork available; mechanism confirmed, capture path not |
| Scoreboard | **Build it** | User's call; treated as regression detection, not truth |
| Serialization | **serde/JSON, encapsulated** | Optional feature + one emitter module, so it can be swapped |
| 7V | **Cheap framing** | 7V multiplexes 4 virtual voices per *hardware* channel; widening arrays buys nothing |
| MIDI mapping key | **(macro, note range, velocity range)** | Confirmed by `$1C`/`$1D`; richer than macro→program |
| Sampler export | **SFZ + DecentSampler `.dspreset`**, behind a serializer trait | Both plain text off the same zone table. SFZ is imported natively by Kontakt, sfizz and most samplers; Kontakt's own `.nki` is ruled out (below) |

### 7V posture (applies to every phase)

`docs/playback-model.md:76-79` and `architecture.md:467-476`: 7V multiplexes four *virtual*
voices per hardware channel in software; Paula still has four channels. Widening the ~47
hard-coded `4`s would be speculative churn. Instead:

- New code takes a voice count and iterates `0..n`, keyed by `u8`. No `; 4]` in new types.
  `TraceEvent` (`tfmx/src/trace.rs`) already does this correctly — follow it.
- **The walker must not inherit `Player::voice_of`'s `& 0x03` mask** (`tfmx/src/player.rs:20`),
  which silently folds 7V nibbles 4–7 onto 0–3. Reporting *raw* nibbles yields a free 7V
  detector: nibbles 4–7 with no 3 is the signature already identifying `apidya (title)`
  (`docs/status.md:1190`).
- A real 7V parser/sequencer stays out of scope, behind the existing register seam.

### Architectural constraints (established, not up for renegotiation)

- `tfmx` core stays dependency-free, I/O-free, thread-free, allocation-free after load and
  `wasm32`-buildable (`docs/architecture.md:18-23`, `:379-383`; empty `[dependencies]` in
  `tfmx/Cargo.toml`). **No new dependency or allocation goes into the core.**
- Trust boundary is exactly `Module`'s accessors — `pattern(n)`, `macro_(n)`,
  `sample(offset, len)`, all bounds-checked returning `Result<_, AccessError>`
  (`architecture.md:286-300`). The walker goes through them and never indexes raw.
- Reuse the two existing seams rather than reaching inside: the **register seam** (`Voice`)
  and the **trace seam** (`TraceEvent`).

### New crate

**`tfmx-analysis`** — library, depends on `tfmx` only, allocation allowed, `serde` an
*optional, default-off* feature. Justified by a second real consumer, not speculation:
`tfmx-cli` is a binary and cannot be depended on, and the deferred web explorer needs the
same walker via `tfmx-web`. Serialization lives in one `serialize` module behind the
feature, so dropping serde leaves the public data structs intact for any other emitter.

## Standing per-phase ritual

Every phase ends with all of these, in one commit:

1. Update the phase's status in the **idea ledger** (`docs/analysis-tooling-ideas.md`).
2. Append to the **session log** (`docs/m5-session-log.md`): what was done, problems hit,
   **mistakes made and how they were resolved**, and anything a future session would
   otherwise re-derive. Write it honestly — wrong turns are the valuable part.
3. Update **ROADMAP.md's Status block** to name the next phase, so a fresh session picks it
   up cold. Tick the phase checkbox in the same commit.
4. `git commit` and `git push`, **then stop.**

## Delegation and model tiers

Per `CLAUDE.md`: hand an agent **only what its subtask needs** — its own block below, the
files it cites, the `docs/` pages it builds on, the hard rules, and its `check:`. Not the
whole plan, not the milestone's reasoning. Reference other docs by path instead of pasting.

| Tier | Use for |
|---|---|
| **Haiku 4.5** | Mechanical, fully specified, single-file, criterion is a passing test |
| **Sonnet 5** | Normal implementation with local design judgment |
| **Opus 5** | Ambiguous, cross-cutting, reverse-engineering, or root-cause work |

## Phases

Each phase: one deliverable, one `check:`, subtasks tagged with their minimum model.

### Phase 5.0 — Idea ledger and session log — **Haiku 4.5**
`docs/analysis-tooling-ideas.md`: every brainstormed idea plus those already in ROADMAP.md,
each with id, title, one-line value, status (`proposed`/`accepted`/`rejected(reason)`/`done`)
and the rationale that moved it. Plus an empty `docs/m5-session-log.md` with its header.
Both referenced from `CLAUDE.md`'s "Where the knowledge lives".

| Subtask | Model |
|---|---|
| Write the ledger from the idea list and decision table in this plan | Haiku 4.5 |
| Create the session-log skeleton and add both `CLAUDE.md` links | Haiku 4.5 |

**check:** all ideas present with a status; the six decisions appear with rationale;
`CLAUDE.md` links both files.

### Phase 5.1 — Reference register-log spike (TIMEBOXED, then re-decide) — **Opus 5**
Determine whether WinUAE 6.1.0's memwatch over `$DFF0A0–$DFF0DF` can *log* audio-register
writes without breaking, while the TFMX editor plays a known module, and whether that output
can be captured to a file. Deliver one captured log plus a parser for its format, **or** a
written negative finding recording exactly what was tried.

Reverse-engineering an undocumented debugger workflow under time pressure — not splittable,
and the phase most likely to need judgment about when to abandon.

**check:** a parsed log of `AUD*PER/VOL/LEN/LC` writes with timestamps exists for one module
— or the negative finding is in the ledger. **Then stop and re-decide** how far oracle work
goes before continuing.

### Phase 5.2 — Static walker core (`tfmx-analysis`) — **Sonnet 5**
Resolve song → reachable trackstep lines → patterns → macros → sample regions, statically,
via `Module`'s bounds-checked accessors. Emits the reachability report (unreachable
patterns/macros, orphaned smpl areas), the mdat byte-provenance map (a free by-product —
patterns and macros have no length field, so provenance *requires* walking to `$F0`/stop),
and raw voice-nibble reporting.

| Subtask | Model |
|---|---|
| Scaffold the crate: manifest, workspace member, optional `serde` feature, empty module tree | Haiku 4.5 |
| Walker traversal + reachable-set collection (the design work) | Sonnet 5 |
| Provenance map derived from traversal spans | Sonnet 5 |
| 7V detector from raw voice nibbles (given the signature, it is a predicate + test) | Haiku 4.5 |

**check:** walks all 10 corpus modules without panic; `apidya (title)` flagged 7V and the
other 9 not; per-module provenance coverage reported. Interpretation note: unclaimed bytes
may be legitimate editor leftovers — the signal is the delta across modules, not 100%.

### Phase 5.3 — Zone resolution (`$1C`/`$1D`) — the spine — **Opus 5**
Per macro, statically resolve `Splitkey`/`Splitvol` branches into
`(note range, velocity range) → sample region + envelope summary`. Everything downstream
consumes this, and getting the interval algebra wrong poisons four exports — worth the tier.

| Subtask | Model |
|---|---|
| Branch resolution and interval splitting | Opus 5 |
| Zone-table data types and their tests | Sonnet 5 |
| Probe macro in `testdata/synth/` with a known split | Haiku 4.5 |

**check:** `turrican intro` macro 28's zones match `tfmx-cli disasm --macro 28`; the probe
macro resolves to exactly two zones at the right boundary.

### Phase 5.4 — JSON dump + serialization seam — **Sonnet 5**
`tfmx-cli dump --format json` over the walker output including 5.3's zone tables, and
`trace --format json` — filling the TODO at `tfmx-cli/src/main.rs:766`, which already
anticipates "a new function plus one `TraceFormat` arm, not a trait". serde stays behind
`tfmx-analysis`'s optional feature; emitters in one module.

| Subtask | Model |
|---|---|
| `serialize` module, feature gating, derives | Sonnet 5 |
| `trace --format json` arm (the seam already exists; this is filling it) | Haiku 4.5 |
| `dump` subcommand wiring | Haiku 4.5 |

**check:** dumps of all 10 modules are valid JSON and re-parse; zone tables present; building
`tfmx-analysis` with `--no-default-features` still compiles and exposes the same structs.

### Phase 5.5 — MIDI export (prioritized) — **Sonnet 5**
`tfmx-cli export-midi` with a JSON mapping file keyed on
`(macro, note range, velocity range)` → `{ program N | drum note N | drop }` plus optional
per-zone transpose, **auto-drafted from 5.3's zones** and hand-editable. Defaults: one MIDI
channel per TFMX voice (1–4, drums→10) because per-channel pitch bend carries
vibrato/portamento; PPQ chosen so 1 jiffy = an exact integer tick count; **no bar
quantization by default**. `midly` (cross-platform, zero required transitive deps, rayon
optional and off) encapsulated behind a single `midi.rs`.

| Subtask | Model |
|---|---|
| Mapping file schema + auto-draft from zone tables | Sonnet 5 |
| Jiffy→PPQ tick math and the note event stream | Sonnet 5 |
| `midly` wrapper module and file writing | Haiku 4.5 |
| Vibrato/portamento → pitch bend | Sonnet 5 |

**check:** a corpus module exports and opens in a cross-platform DAW/player; note count and
onset timing match the trace's `Trigger` events; editing the mapping changes the output.
Bonus: MIDI export does not depend on sample fidelity, so it is an *independent* ear-oracle
— the open `MIDDLE_C_NOTE` pitch question gets a cheap probe here.

### Phase 5.6 — Fidelity scoreboard — **Sonnet 5**
Batch-render the corpus, compute distance metrics against reference material, store as a
tracked metrics file so changes move a number. Uses register-log comparison if 5.1 succeeded,
audio metrics otherwise. Reuses existing `onset-diff` and `measure-pitch`.

| Subtask | Model |
|---|---|
| Metric computation and the tracked metrics file | Sonnet 5 |
| Batch runner over the corpus | Haiku 4.5 |

**check:** runs over all 10 modules and is committed; a deliberate known-bad mutation moves
the metric. **Honesty requirement, recorded in the ledger:** this project's history is
structural metrics moving while the ear did not — the scoreboard is regression detection,
not a truth oracle, and must be labelled as such wherever it is reported.

### Phase 5.7 — Sample and sampler-instrument export — **Sonnet 5**

**Structure first: one `InstrumentSerializer` trait over 5.3's zone table**, with each format
an independent implementation registered by name (`--format sfz|dspreset|...`). This is the
one place in the milestone where an abstraction is warranted rather than speculative — it has
multiple implementations on day one and is explicitly meant to grow. Adding a format later
must be one new file implementing the trait plus one registry line, touching nothing else.

Formats shipped:

- **WAV with a `smpl` loop chunk** — loop points survive into hardware/software samplers.
- **SFZ** — `lokey`/`hikey`/`lovel`/`hivel` map one-to-one from the zone table. This is the
  universal path: imported natively by **Kontakt**, and playable in **sfizz** (free
  VST3/AU/LV2 for macOS/Linux/Windows), which is the route into Ableton Live.
- **DecentSampler `.dspreset`** — plain XML, free cross-platform plugin, keeps key/velocity
  ranges and loop points. The zero-friction "drop it in and play" path.

**Ruled out — do not attempt:** Kontakt `.nki`. Since Kontakt 4.2 it is a binary format with
128-bit encryption; `nkitool` supports only v1–v4 pre-4.2, and `monomadic/ni-file` is
incomplete reverse engineering of a still-encrypted target. Moot anyway, since Kontakt
imports SFZ. Ableton `.adg` likewise: gzipped, undocumented, version-tied — and covered by
SFZ-via-sfizz. Both are recorded in the ledger as `rejected` with these reasons so a future
session does not re-litigate them.

| Subtask | Model |
|---|---|
| `InstrumentSerializer` trait, registry, and format dispatch | Sonnet 5 |
| WAV `smpl` chunk writer | Haiku 4.5 |
| SFZ implementation | Haiku 4.5 |
| DecentSampler `.dspreset` implementation | Sonnet 5 |

**check:** the exported SFZ loads in sfizz *and* imports into Kontakt, with zone boundaries
matching the dump's; the `.dspreset` loads in DecentSampler and plays across its key ranges;
exported WAV loop points round-trip; adding a stub fourth format requires no edit outside its
own file and the registry.

### Phase 5.8 — Visualization — **Sonnet 5**
**The data collection is the deliverable; the HTML is one consumer.** Build view-model
structs in `tfmx-analysis` (serializable via 5.4's seam) describing what is to be shown —
waveform regions and loop points, the pattern→macro graph, the trackstep structure map — and
keep the renderer a thin, replaceable function over them. A later GUI, an export, or further
processing must be able to consume the same structs without touching the HTML path.

| Subtask | Model |
|---|---|
| View-model structs + their JSON serialization (the actual seam) | Sonnet 5 |
| Self-contained HTML renderer over the view models | Sonnet 5 |
| Mermaid call-graph emitter (pure string building from the graph struct) | Haiku 4.5 |

**check:** renders for all 10 modules; the out-of-bounds regions `lint` reports are visibly
marked on the waveform view; the view-model JSON is consumable standalone, with no HTML in
`tfmx-analysis`.

## Deferred to the ledger, not planned here

Probe-module generator, Amiga plausibility check, opcode corpus census, static conformance
linter, smpl directory reconstruction, cross-module macro fingerprinting, static loop/duration
detection, interactive trace explorer, spectrogram comparison, web module explorer, tracker
(XM/MOD/IT) export, score/notation export. All recorded in Phase 5.0's ledger as `proposed`
so a later session can pick any of them up cold.

## Verification

- `cargo test --workspace`, `cargo clippy --workspace`, and `tfmx/tests/mutation_robustness.rs`
  pass throughout.
- `tfmx/Cargo.toml`'s `[dependencies]` stays empty and
  `cargo build -p tfmx --target wasm32-unknown-unknown` still succeeds — the core-crate rule
  is mechanically checkable, so check it mechanically.
- `tfmx-cli/tests/golden.rs` hashes must **not** change: this milestone adds static analysis
  and export and touches no playback path. A changed golden hash means something leaked into
  the renderer and is a bug in this milestone.
- Per-phase `check:` criteria above, plus the standing per-phase ritual (ledger, session log,
  ROADMAP Status, commit, push, stop).
- TDD per the standing rule: failing tests before implementation, on top of each `check:`.
