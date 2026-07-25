# tfmx-rs

A Rust library and cross-platform player for **TFMX** (*The Final Musicsystem eXtended*),
the Amiga music format created by Chris Hülsbeck — the engine behind Turrican, Apidya,
R-Type and many other titles. TFMX modules come as a pair of files: `mdat.*` (song, patterns,
macros) and `smpl.*` (sample data).

## Status

Early development. See [`docs/`](docs/) for the format documentation and
[the plan](#milestones) below for where things stand.

## Layout

| Crate | Purpose |
|---|---|
| `tfmx` | Core library: parser, sequencer, Paula software mixer. No I/O, no threads, no dependencies. |
| `tfmx-cli` | Command line front end: render a module to WAV, inspect its structure. |

The core crate is dependency-free and builds unchanged for `wasm32-unknown-unknown`, so the
same decoder drives desktop playback and a Web Audio worklet.

## Milestones

- **M1** — documentation, parser, 4-channel renderer, WAV output *(in progress)*
- **M2** — desktop realtime playback (`cpal`)
- **M3** — web player (wasm + AudioWorklet)
- Later — TFMX 7V support, GemX macro opcodes, tooling

## Documentation

- [`docs/format.md`](docs/format.md) — on-disk data model
- [`docs/opcodes.md`](docs/opcodes.md) — complete command reference
- [`docs/playback-model.md`](docs/playback-model.md) — how sound is produced, and the gotchas
- [`docs/architecture.md`](docs/architecture.md) — code shape and design decisions

## Provenance and licensing

This implementation is written **from the published format specification**, primarily
[*The TFMX Professional 2.0 Song File Format*](https://github.com/libxmp/libxmp/blob/master/docs/formats/tfmx-format.txt)
by Jonathan H. Pickard, together with the
[playback-tfmx notes](https://github.com/RetrovertApp/playback-tfmx/blob/master/TFMX.md).

Every pre-existing TFMX replayer we are aware of (UADE, libtfmxaudiodecoder, playback-tfmx,
foo_input_tfmx) is GPL-2.0. **No GPL source was consulted while writing this code.** Those
players are used only as black boxes — executed to produce reference audio for A/B listening,
never read. That is what keeps this crate usable under a permissive licence.

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The music modules themselves are copyrighted works of their respective composers and are not
distributed with this repository.
