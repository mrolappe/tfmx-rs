// AudioWorkletGlobalScope has no TextDecoder (unlike Window or dedicated
// workers) -- wasm-bindgen's generated glue constructs one eagerly at
// module load to decode error strings out of wasm memory, so importing it
// there throws before `registerProcessor` ever runs. Importing *this* file
// first (it has no imports of its own, so it's guaranteed to finish
// evaluating first) defines a minimal UTF-8-decoding stand-in.
export class MinimalTextDecoder {
  // wasm-bindgen also calls `decode()` with no arguments once as a V8 JIT
  // warm-up; `bytes` is `undefined` in that case.
  decode(bytes) {
    if (bytes === undefined) {
      return '';
    }
    let result = '';
    for (let i = 0; i < bytes.length; ) {
      const b0 = bytes[i++];
      if (b0 < 0x80) {
        result += String.fromCharCode(b0);
      } else if (b0 < 0xe0) {
        const b1 = bytes[i++];
        result += String.fromCharCode(((b0 & 0x1f) << 6) | (b1 & 0x3f));
      } else if (b0 < 0xf0) {
        const b1 = bytes[i++];
        const b2 = bytes[i++];
        result += String.fromCharCode(((b0 & 0x0f) << 12) | ((b1 & 0x3f) << 6) | (b2 & 0x3f));
      } else {
        const b1 = bytes[i++];
        const b2 = bytes[i++];
        const b3 = bytes[i++];
        const cp =
          (((b0 & 0x07) << 18) | ((b1 & 0x3f) << 12) | ((b2 & 0x3f) << 6) | (b3 & 0x3f)) - 0x10000;
        result += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
      }
    }
    return result;
  }
}

if (typeof TextDecoder === 'undefined') {
  globalThis.TextDecoder = MinimalTextDecoder;
}
