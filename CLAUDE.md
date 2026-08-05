# Project notes

A Rust library and cross-platform player for the Amiga **TFMX** music format.

## Resuming work

**[ROADMAP.md](ROADMAP.md) is the progress tracker and the authoritative step list.** Read its
**Status block at the top** — it names the next step, the current phase, and whether that phase
has been approved yet. Each step carries its deliverable, its verification and a recommended
minimum model. Tick the checkbox *and* update the Status block in the same commit that
completes the step.

**Turnaround loop, every completed step, every session, no exceptions**: tick the checkbox, update
the Status block, commit, **push**, **then stop** so the user can start a fresh session. Do not
treat "commit" as satisfying this on its own -- an uncommitted or unpushed step is not done.

`git log` is the record of what happened — commit subjects name their step, e.g. `(step 0.3)`.

## Delegating to agents

Give an agent **only what its step needs**: the step's own block from ROADMAP.md, the sources
it cites, the `docs/` files it builds on, the hard rules below, and its verification criterion.
Do not hand over the whole roadmap, the plan history, or earlier steps' reasoning. If a step
cannot stand on its own as a brief, sharpen the step rather than widening the context. See
"Delegating a step" in ROADMAP.md.

## Hard rules

- **Never read GPL source.** Every existing TFMX replayer (UADE, libtfmxaudiodecoder,
  playback-tfmx, foo_input_tfmx) is GPL-2.0. This crate is written from the published format
  spec so it can stay MIT/Apache-2.0. Reference players may be *executed* to produce audio for
  A/B comparison — their code is off limits, and so is anything derived from reading it.
- **Stop at phase gates.** At the end of every phase in ROADMAP.md, stop and wait for explicit
  approval before starting the next.
- English for all code, comments, docs, commit messages and file names.
- The `tfmx` core crate stays dependency-free, allocation-free after load, and free of I/O and
  threads — it must build unchanged for `wasm32-unknown-unknown`.
- `testdata/` holds copyrighted music and is gitignored. `testdata/fetch.sh` obtains it.

## Where the knowledge lives

- [`docs/format.md`](docs/format.md) — on-disk data model
- [`docs/opcodes.md`](docs/opcodes.md) — complete command reference
- [`docs/playback-model.md`](docs/playback-model.md) — how sound is produced, plus the gotchas
- [`docs/architecture.md`](docs/architecture.md) — code shape and design decisions
- [`docs/replayer-walkthrough.md`](docs/replayer-walkthrough.md) — narrative walkthrough of
  `Player::run_jiffy`'s exact order of operations, tying trackstep/pattern/macro/opcode
  execution together with diagrams and worked examples; read this first for *order*, the
  three docs above for *bytes*, *math* and *code shape*
- [`docs/m5-plan.md`](docs/m5-plan.md) — the approved M5 "Export and static analysis" brief:
  decisions and their rationale, per-phase subtasks and minimum models, the 7V posture
- [`docs/analysis-tooling-ideas.md`](docs/analysis-tooling-ideas.md) — M5's idea ledger: every
  brainstormed idea with its status and rationale
- [`docs/m5-session-log.md`](docs/m5-session-log.md) — M5's per-phase session log: what was
  done, problems hit, mistakes made and how they were resolved
- [`ROADMAP-history.md`](ROADMAP-history.md) — completed milestones' full step detail and
  findings, split out of ROADMAP.md so it isn't loaded by default; pull it up for a step's
  rationale or a past gotcha

Prefer these over re-deriving anything from the upstream spec; they exist so the format
knowledge is not trapped in the code.
