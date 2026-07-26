// Converts interleaved i16 PCM (what tfmx-web's `render()` produces) into
// planar f32 per channel (what `AudioWorkletProcessor.process()`'s output
// array wants). Fills `channels` in place -- no allocation, mirroring the
// no-alloc-per-block convention `render()` itself follows.
//
// Loaded two ways: `require()` under Node (this test) and `importScripts()`
// in the AudioWorkletGlobalScope (step 9.2), which has no `module` global --
// hence the guarded export at the bottom instead of a bare CommonJS export.

// Scale by 32768, not 32767: matches `tfmx-play`'s `i16_to_f32` (step 7.1)
// so both playback paths treat full-scale negative (-32768) as exactly -1.0.
function interleavedI16ToPlanarF32(interleaved, channels) {
  const channelCount = channels.length;
  const frameCount = channels[0].length;
  for (let frame = 0; frame < frameCount; frame++) {
    for (let ch = 0; ch < channelCount; ch++) {
      channels[ch][frame] = interleaved[frame * channelCount + ch] / 32768;
    }
  }
}

if (typeof module !== 'undefined') {
  module.exports = { interleavedI16ToPlanarF32 };
}
