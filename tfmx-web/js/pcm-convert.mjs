// Converts interleaved i16 PCM (what tfmx-web's `render()` produces) into
// planar f32 per channel (what `AudioWorkletProcessor.process()`'s output
// array wants). Fills `channels` in place -- no allocation, mirroring the
// no-alloc-per-block convention `render()` itself follows.
//
// `.mjs` + `export`, not the guarded CommonJS pattern this file started
// with: AudioWorklet processor modules are always ES modules (no
// `importScripts`, unlike dedicated/shared workers -- confirmed against a
// real browser), so the 9.2 processor needs a real `import`, and Node picks
// its module system from this file's own extension either way.

// Scale by 32768, not 32767: matches `tfmx-play`'s `i16_to_f32` (step 7.1)
// so both playback paths treat full-scale negative (-32768) as exactly -1.0.
export function interleavedI16ToPlanarF32(interleaved, channels) {
  const channelCount = channels.length;
  const frameCount = channels[0].length;
  for (let frame = 0; frame < frameCount; frame++) {
    for (let ch = 0; ch < channelCount; ch++) {
      channels[ch][frame] = interleaved[frame * channelCount + ch] / 32768;
    }
  }
}
