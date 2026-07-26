#!/usr/bin/env sh
# Builds the browser-loadable wasm + JS glue for tfmx-web's AudioWorklet
# integration (step 9.2). `--target web`: AudioWorklet processor modules are
# always loaded as ES modules per the Worklet spec (no `importScripts`,
# unlike dedicated/shared workers -- confirmed against a real browser, the
# ROADMAP's original "no-modules + importScripts" note was wrong), so the
# glue has to be a real ES module too. Output goes to js/generated/,
# gitignored like target/ itself.
set -eu
cd "$(dirname "$0")/.."

cargo build -p tfmx-web --release --target wasm32-unknown-unknown --target-dir target/wasm

wasm-bindgen --target web --out-dir tfmx-web/js/generated --out-name tfmx_web \
  target/wasm/wasm32-unknown-unknown/release/tfmx_web.wasm
