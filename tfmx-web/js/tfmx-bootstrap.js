// Main-thread half of the AudioWorklet setup (step 9.2): compile the wasm
// once here (WebAssembly.compileStreaming), register the processor module,
// then hand the compiled module plus the file bytes across the worklet's
// port -- the worklet's `initSync` takes an already-compiled module instead
// of re-fetching/re-compiling in its own scope.
async function createTfmxWorkletNode(
  audioContext,
  { wasmUrl, processorUrl, mdat, smpl, onStatus = () => {} },
) {
  onStatus('compiling wasm...');
  const module = await WebAssembly.compileStreaming(fetch(wasmUrl));
  onStatus('adding worklet module...');
  await audioContext.audioWorklet.addModule(processorUrl);
  onStatus('creating worklet node...');

  const node = new AudioWorkletNode(audioContext, 'tfmx-processor', {
    numberOfInputs: 0,
    numberOfOutputs: 1,
    outputChannelCount: [2],
  });

  // Connect before the handshake, not after: a worklet's incoming message
  // queue is only pumped while its process() is actually being called, and
  // process() only runs once the node has a live path to the destination.
  // Sending `init` to a not-yet-connected node deadlocks -- process() never
  // ran, so it's harmless that `this.tfmx` is still null at this point (see
  // the processor's own guard).
  node.connect(audioContext.destination);

  // The processor's message port isn't guaranteed to be listening the
  // instant the node is constructed -- a message posted immediately here
  // gets silently dropped rather than queued (observed in Chrome). Wait for
  // the processor's own "ready" ping, sent once its constructor has run and
  // `port.onmessage` is actually attached, before sending init data.
  onStatus('waiting for processor ready ping...');
  await withTimeout(
    new Promise((resolve) => {
      node.port.onmessage = (event) => {
        if (event.data.type === 'ready') resolve();
      };
    }),
    'waiting for processor ready ping',
  );

  onStatus('sending init, waiting for init-ok...');
  await withTimeout(
    new Promise((resolve, reject) => {
      node.port.onmessage = (event) => {
        if (event.data.type === 'init-ok') resolve();
        else if (event.data.type === 'init-error') reject(new Error(event.data.message));
      };
      node.port.postMessage({ type: 'init', module, mdat, smpl, sampleRate: audioContext.sampleRate });
    }),
    'waiting for init-ok/init-error',
  );

  return node;
}

// A hung port handshake should fail loudly, not hang the caller forever.
function withTimeout(promise, label, ms = 3000) {
  return Promise.race([
    promise,
    new Promise((_, reject) => setTimeout(() => reject(new Error(`timed out: ${label}`)), ms)),
  ]);
}

if (typeof module !== 'undefined') {
  module.exports = { createTfmxWorkletNode };
}
