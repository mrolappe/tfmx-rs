// Demo page wiring (step 10.1): drop zone -> pair mdat/smpl -> worklet node
// from tfmx-bootstrap.js (loaded as a plain script, see index.html) ->
// play/pause and song-select controls.
import { pairFiles } from './pair-files.mjs';

// The header's song-slot table is always 32 entries wide (docs/format.md).
const SONG_SLOTS = 32;

const dropZone = document.getElementById('drop-zone');
const fileInput = document.getElementById('file-input');
const status = document.getElementById('status');
const controls = document.getElementById('controls');
const playPause = document.getElementById('play-pause');
const songSelect = document.getElementById('song-select');

for (let i = 0; i < SONG_SLOTS; i++) {
  const option = document.createElement('option');
  option.value = i;
  option.textContent = `${i}`;
  songSelect.appendChild(option);
}

// Created once and reused for every subsequent file drop -- closing and
// re-creating an AudioContext per load raced with Safari's (slower, more
// limited) hardware teardown and made later loads hang forever on
// `resume()`. A single long-lived context, with a fresh worklet node
// swapped in per load, is also the pattern MDN itself recommends.
let audioContext = null;
let currentNode = null;

function setStatus(message, isError = false) {
  status.textContent = message;
  status.classList.toggle('error', isError);
}

// Safari ties Web Audio's autoplay permission to the *direct* user gesture.
// `resume()` must run synchronously inside the click/drop handler itself --
// calling it later, e.g. from the file input's `change` event (which fires
// only after the native file-picker dialog closes), is one step removed
// from the actual gesture and Safari never grants it (confirmed: it hangs
// there specifically, only in Safari, every time).
async function ensureAudioContextResumed() {
  audioContext ??= new AudioContext();
  await audioContext.resume();
}

async function loadPair(mdatFile, smplFile) {
  setStatus(`reading ${mdatFile.name}...`);
  const [mdat, smpl] = await Promise.all([
    mdatFile.arrayBuffer().then((b) => new Uint8Array(b)),
    smplFile.arrayBuffer().then((b) => new Uint8Array(b)),
  ]);

  currentNode?.disconnect();

  const node = await createTfmxWorkletNode(audioContext, {
    wasmUrl: '../js/generated/tfmx_web_bg.wasm',
    processorUrl: '../js/tfmx-processor.js',
    mdat,
    smpl,
    onStatus: setStatus,
  });
  currentNode = node;

  songSelect.value = '0';
  songSelect.onchange = () => {
    node.port.postMessage({ type: 'set-song', song: Number(songSelect.value) });
  };

  playPause.textContent = 'Pause';
  playPause.onclick = async () => {
    // A second click while the first toggle is still in flight must not
    // spawn a second overlapping suspend()/resume() race.
    playPause.disabled = true;
    try {
      if (audioContext.state === 'running') {
        await audioContext.suspend();
        playPause.textContent = 'Play';
      } else {
        await audioContext.resume();
        playPause.textContent = 'Pause';
      }
    } finally {
      playPause.disabled = false;
    }
  };

  controls.hidden = false;
  setStatus(`playing ${mdatFile.name}`);
}

async function handleFiles(files) {
  try {
    const { mdatFile, smplFile } = pairFiles(files);
    await loadPair(mdatFile, smplFile);
  } catch (e) {
    setStatus(String(e.message ?? e), true);
  }
}

// `fileInput.click()` -- like `audioContext.resume()` -- needs live transient
// user activation. That activation is consumed by the first gesture-gated
// call made after the event; awaiting `ensureAudioContextResumed()` first
// (as this used to) spent it before `.click()` ever ran, so the picker
// often didn't open at all (confirmed: needed several clicks in Safari).
// `.click()` must run synchronously, first; the context can resume
// independently right after, not gating it.
dropZone.addEventListener('click', () => {
  fileInput.click();
  ensureAudioContextResumed().catch((e) => setStatus(String(e.message ?? e), true));
});
dropZone.addEventListener('keydown', (e) => {
  if (e.key !== 'Enter' && e.key !== ' ') return;
  fileInput.click();
  ensureAudioContextResumed().catch((e) => setStatus(String(e.message ?? e), true));
});
fileInput.addEventListener('change', () => handleFiles([...fileInput.files]));

dropZone.addEventListener('dragover', (e) => {
  e.preventDefault();
  dropZone.classList.add('drag-over');
});
dropZone.addEventListener('dragleave', () => dropZone.classList.remove('drag-over'));
dropZone.addEventListener('drop', (e) => {
  e.preventDefault();
  dropZone.classList.remove('drag-over');
  ensureAudioContextResumed().catch((e) => setStatus(String(e.message ?? e), true));
  handleFiles([...e.dataTransfer.files]);
});
