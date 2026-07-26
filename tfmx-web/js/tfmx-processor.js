// AudioWorkletProcessor loaded via `audioContext.audioWorklet.addModule()`.
// Per the Worklet spec, that always loads the file as an ES module (no
// `importScripts`, unlike dedicated/shared workers) -- so this pulls in the
// wasm-bindgen `--target web` glue (build-wasm.sh's output) and step 9.1's
// converter via ordinary static `import`.
//
// Must come first: the generated glue below constructs a `TextDecoder` at
// module-evaluation time, and this scope has none (see the polyfill file).
import './text-decoder-polyfill.mjs';
import { initSync, TfmxWeb } from './generated/tfmx_web.js';
import { interleavedI16ToPlanarF32 } from './pcm-convert.mjs';

class TfmxProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.tfmx = null;
    // Reused across process() calls -- sized once the first real block
    // arrives, matching the render-quantum's fixed 128 frames.
    this.interleaved = new Int16Array(0);
    this.port.onmessage = (event) => this.handleMessage(event.data);
    // Tells the main thread (see tfmx-bootstrap.js) it's now safe to send
    // `init` -- see the constructor-vs-port-readiness comment there.
    this.port.postMessage({ type: 'ready' });
  }

  handleMessage(message) {
    if (message.type === 'init') {
      try {
        initSync({ module: message.module });
        this.tfmx = new TfmxWeb(
          new Uint8Array(message.mdat),
          new Uint8Array(message.smpl),
          message.sampleRate,
        );
        this.port.postMessage({ type: 'init-ok' });
      } catch (e) {
        this.port.postMessage({ type: 'init-error', message: String(e) });
      }
    } else if (message.type === 'set-song') {
      this.tfmx?.set_song(message.song);
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!this.tfmx || output.length === 0) {
      return true;
    }

    const needed = output[0].length * output.length;
    if (this.interleaved.length !== needed) {
      this.interleaved = new Int16Array(needed);
    }

    this.tfmx.render(this.interleaved);
    interleavedI16ToPlanarF32(this.interleaved, output);
    return true;
  }
}

registerProcessor('tfmx-processor', TfmxProcessor);
