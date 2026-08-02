# M5 idea ledger

Every idea brainstormed for the "Export and static analysis" milestone, plus the ones already
committed to [ROADMAP.md](../ROADMAP.md)'s M5 phase list. Status is one of `proposed`,
`accepted`, `rejected(reason)`, `done`. Update this file as part of the standing per-phase
ritual in [`docs/m5-plan.md`](m5-plan.md).

## Decisions (2026-08-02)

The six calls that shaped the milestone, from `m5-plan.md`'s decision table:

| Decision | Choice | Rationale |
|---|---|---|
| Oracle | Timebox a spike, then re-decide | WinUAE fork available; mechanism confirmed, capture path not |
| Scoreboard | Build it | User's call; treated as regression detection, not truth |
| Serialization | serde/JSON, encapsulated | Optional feature + one emitter module, so it can be swapped |
| 7V | Cheap framing | 7V multiplexes 4 virtual voices per *hardware* channel; widening arrays buys nothing |
| MIDI mapping key | (macro, note range, velocity range) | Confirmed by `$1C`/`$1D`; richer than macro→program |
| Sampler export | SFZ + DecentSampler `.dspreset`, behind a serializer trait | Both plain text off the same zone table. SFZ is imported natively by Kontakt, sfizz and most samplers; Kontakt's own `.nki` is ruled out |

## Accepted — planned as M5 phases

| ID | Title | Value | Status | Rationale |
|---|---|---|---|---|
| M5-01 | Idea ledger + session log skeleton | Gives every later phase a place to record status and history without re-deriving it | `accepted` (this phase) | Needed before any other phase can follow the standing ritual |
| M5-02 | Reference register-log spike (WinUAE memwatch over `$DFF0A0–$DFF0DF`) | Cheap oracle captured from the TFMX editor itself, replacing costly user-listening verification rounds | `done`, phase 5.1 | **Positive result.** `Memwatch <n>: break at <addr>.<size> <RWI> <value> PC=<pc>` lines, decodable to `AUD0-3 LCH/LCL/LEN/PER/VOL`, appear in the debugger console but not in redirected stdout — must be copied out of the console window by hand. A second `logonly` watch on `$DFF09C` (INTREQ) catches the OS's once-per-frame VERTB write and supplies jiffy-resolution relative timing, closing the format's missing-timestamp gap. See `docs/m5-session-log.md`'s Phase 5.1 entry for the full recipe and decoded sample. |
| M5-03 | Static walker core (`tfmx-analysis` crate) | Resolves song → reachable trackstep lines → patterns → macros → sample regions statically; yields reachability report, mdat byte-provenance map, raw voice-nibble reporting | `done`, phase 5.2 | Walks all 10 corpus modules (song 0) without panic; `apidya (title)` uniquely flagged 7V; per-module provenance coverage 16.7–60.9%. See `docs/m5-session-log.md`'s Phase 5.2 entry. |
| M5-04 | Zone resolution (`$1C`/`$1D`) | Per-macro `(note range, velocity range) → sample region` table — the spine feeding MIDI, sampler, sample and tracker export | `done`, phase 5.3 | `resolve_zones` in `tfmx-analysis/src/zones.rs`; `turrican intro` macro 28 and a synthetic probe both check out. Open finding, not acted on: macro 5's `$1D` chain reads as dead code under the documented "jump if volume < aa" polarity but as a clean 5-way velocity fan-out under the reverse — see `docs/m5-session-log.md`'s Phase 5.3 entry. |
| M5-05 | JSON dump + serialization seam | `tfmx-cli dump --format json` and `trace --format json`, filling the existing TODO at `tfmx-cli/src/main.rs:766` | `accepted`, phase 5.4 | serde stays optional and encapsulated per the serialization decision above |
| M5-06 | MIDI export | `tfmx-cli export-midi` with an auto-drafted, hand-editable mapping file; independent ear-oracle for the open `MIDDLE_C_NOTE` pitch question | `accepted`, phase 5.5, user-prioritized | Does not depend on sample fidelity, unlike every prior verification path in this project |
| M5-07 | Fidelity scoreboard | Batch-rendered distance metrics against reference material, tracked as a committed metrics file | `accepted`, phase 5.6 | Explicitly regression detection, not a truth oracle — this project's history shows structural metrics moving while the ear did not |
| M5-08 | Sample + sampler-instrument export | WAV with `smpl` loop chunk, SFZ, DecentSampler `.dspreset`, behind one `InstrumentSerializer` trait | `accepted`, phase 5.7 | Only place in the milestone where an abstraction is warranted — multiple implementations on day one, meant to grow |
| M5-09 | Visualization | View-model structs (waveform regions, loop points, pattern→macro graph, trackstep structure map) with a thin HTML renderer as one consumer | `accepted`, phase 5.8 | Data collection is the deliverable; the HTML is replaceable |

## Rejected

| ID | Title | Value | Status | Rationale |
|---|---|---|---|---|
| M5-R1 | Kontakt `.nki` export | Native Kontakt instrument format | `rejected(binary+encrypted since Kontakt 4.2; nkitool only covers pre-4.2; no complete reverse engineering exists; moot since Kontakt imports SFZ natively)` | Recorded so a future session does not re-litigate it |
| M5-R2 | Ableton `.adg` export | Native Ableton device group format | `rejected(gzipped, undocumented, version-tied; covered by SFZ-via-sfizz, which is Ableton's own route into third-party samplers)` | Recorded so a future session does not re-litigate it |

## Proposed — deferred, not planned

Everything from `m5-plan.md`'s "Deferred to the ledger" list. No active lead on any of these;
pick any of them up cold in a later session.

| ID | Title | Value | Status |
|---|---|---|---|
| M5-P1 | Probe-module generator | Synthetic modules with known-in-advance structure, for testing the walker/zone resolver against ground truth instead of only the real corpus | `proposed` |
| M5-P2 | Amiga plausibility check | Flags module data that is structurally valid but physically implausible on real Amiga hardware (e.g. DMA/timing limits) | `proposed` |
| M5-P3 | Opcode corpus census | Statistics on which opcodes/operand ranges actually occur across the ten corpus modules, informing which `Uncertain` doc items are worth resolving next | `proposed` |
| M5-P4 | Static conformance linter | Extends `tfmx-cli lint`'s existing runtime findings with additional static-only checks the walker can now afford | `proposed` |
| M5-P5 | smpl directory reconstruction | Best-effort recovery of a named sample directory from macro-referenced regions, since the `smpl` file itself has none | `proposed` |
| M5-P6 | Cross-module macro fingerprinting | Detects macros reused verbatim (or near-verbatim) across different modules/composers | `proposed` |
| M5-P7 | Static loop/duration detection | Determines whether a song loops indefinitely or terminates, and its structural duration, without rendering audio | `proposed` |
| M5-P8 | Interactive trace explorer | Browser-based step-through of a trace, beyond the static HTML views in phase 5.8 | `proposed` |
| M5-P9 | Spectrogram comparison | Visual/frequency-domain diffing between this crate's render and a reference render | `proposed` |
| M5-P10 | Web module explorer | Browser UI over `tfmx-web`, consuming the same walker output as the CLI tools | `proposed` |
| M5-P11 | Tracker (XM/MOD/IT) export | Converts a resolved song into a tracker format for editing in tools outside this project | `proposed` |
| M5-P12 | Score/notation export | Converts a resolved song into conventional music notation | `proposed` |
