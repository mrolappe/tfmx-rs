# Project notes

A Rust library and cross-platform player for the Amiga **TFMX** music format.

## Resuming work

**[ROADMAP.md](ROADMAP.md) is the progress tracker and the authoritative step list.** Read its
**Status block at the top** — it names the next step, the current phase, and whether that phase
has been approved yet. Each step carries its deliverable, its verification and a recommended
minimum model. Tick the checkbox *and* update the Status block in the same commit that
completes the step.

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

Prefer these over re-deriving anything from the upstream spec; they exist so the format
knowledge is not trapped in the code.
