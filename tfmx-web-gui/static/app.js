// Frontend wiring over tfmx-web-gui's routes (docs/gui-plan.md Phase W2/W3):
// list/load a module, then browse it via the song-view iframe (which plays
// waveform regions itself, see visualize.rs's REGION_SCRIPT), rendered
// audio, and a disasm dump. Song/pattern/macro pickers and counts come from
// /module-info, fetched once right after /load succeeds.

const status = document.getElementById('status');
const dirInput = document.getElementById('dir-input');
const fileSelect = document.getElementById('file-select');
const moduleInfoBox = document.getElementById('module-info');
const songCount = document.getElementById('song-count');
const patternCount = document.getElementById('pattern-count');
const macroCount = document.getElementById('macro-count');
const songSection = document.getElementById('song-section');
const renderSection = document.getElementById('render-section');
const disasmSection = document.getElementById('disasm-section');
const songSelect = document.getElementById('song-select');
const songViewFrame = document.getElementById('song-view-frame');
const macroSelect = document.getElementById('macro-select');
const patternSelect = document.getElementById('pattern-select');
const patternTempo = document.getElementById('pattern-tempo');
const macroAudio = document.getElementById('macro-audio');
const patternAudio = document.getElementById('pattern-audio');
const disasmKind = document.getElementById('disasm-kind');
const disasmNumber = document.getElementById('disasm-number');
const disasmTabJson = document.getElementById('disasm-tab-json');
const disasmTabText = document.getElementById('disasm-tab-text');
const disasmOutputJson = document.getElementById('disasm-output-json');
const disasmOutputText = document.getElementById('disasm-output-text');

let moduleInfo = null;

function setStatus(message, isError = false) {
  status.textContent = message;
  status.classList.toggle('error', isError);
}

async function fetchJson(url, options) {
  const response = await fetch(url, options);
  const body = await response.json();
  if (!response.ok) throw new Error(body.error ?? `request to ${url} failed`);
  return body;
}

function fillSelect(select, numbers) {
  select.innerHTML = '';
  for (const n of numbers) {
    const option = document.createElement('option');
    option.value = n;
    option.textContent = n;
    select.appendChild(option);
  }
}

document.getElementById('list-files').addEventListener('click', async () => {
  try {
    setStatus('listing...');
    const pairs = await fetchJson(`/files?dir=${encodeURIComponent(dirInput.value)}`);
    fileSelect.innerHTML = '';
    for (const pair of pairs) {
      const option = document.createElement('option');
      option.value = JSON.stringify({ mdat_path: pair.mdat_path, smpl_path: pair.smpl_path });
      option.textContent = pair.name;
      fileSelect.appendChild(option);
    }
    setStatus(`found ${pairs.length} module(s)`);
  } catch (e) {
    setStatus(e.message, true);
  }
});

document.getElementById('load').addEventListener('click', async () => {
  if (!fileSelect.value) {
    setStatus('list a directory and pick a module first', true);
    return;
  }
  try {
    setStatus('loading...');
    await fetchJson('/load', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: fileSelect.value,
    });

    moduleInfo = await fetchJson('/module-info');
    songCount.textContent = moduleInfo.songs.length;
    patternCount.textContent = moduleInfo.patterns.length;
    macroCount.textContent = moduleInfo.macros.length;
    moduleInfoBox.hidden = false;

    songSelect.innerHTML = '';
    for (const song of moduleInfo.songs) {
      const option = document.createElement('option');
      option.value = song.number;
      option.textContent = `${song.number} (start ${song.start}, end ${song.end}, tempo ${song.tempo})`;
      songSelect.appendChild(option);
    }
    fillSelect(macroSelect, moduleInfo.macros);
    fillSelect(patternSelect, moduleInfo.patterns);
    updateDisasmNumberOptions();
    updatePatternTempoDefault();

    songSection.hidden = false;
    renderSection.hidden = false;
    disasmSection.hidden = false;
    setStatus(`loaded ${fileSelect.options[fileSelect.selectedIndex].textContent}`);
  } catch (e) {
    setStatus(e.message, true);
  }
});

function updatePatternTempoDefault() {
  const song = moduleInfo?.songs.find((s) => String(s.number) === songSelect.value);
  if (song) patternTempo.value = song.tempo;
}

songSelect.addEventListener('change', updatePatternTempoDefault);

document.getElementById('view-song').addEventListener('click', () => {
  songViewFrame.src = `/song-view.html?song=${songSelect.value}`;
});

document.getElementById('render-macro-btn').addEventListener('click', () => {
  const macro = macroSelect.value;
  const note = document.getElementById('macro-note').value;
  const volume = document.getElementById('macro-volume').value;
  macroAudio.src = `/render-macro?macro=${macro}&note=${note}&volume=${volume}`;
});

document.getElementById('render-pattern-btn').addEventListener('click', () => {
  const pattern = patternSelect.value;
  const transpose = document.getElementById('pattern-transpose').value;
  const tempo = patternTempo.value;
  patternAudio.src = `/render-pattern?pattern=${pattern}&transpose=${transpose}&tempo=${tempo}`;
});

function updateDisasmNumberOptions() {
  const numbers = disasmKind.value === 'macro' ? moduleInfo.macros : moduleInfo.patterns;
  fillSelect(disasmNumber, numbers);
}

disasmKind.addEventListener('change', updateDisasmNumberOptions);

function showDisasmTab(tab) {
  const json = tab === 'json';
  disasmTabJson.classList.toggle('active', json);
  disasmTabText.classList.toggle('active', !json);
  disasmOutputJson.hidden = !json;
  disasmOutputText.hidden = json;
}

disasmTabJson.addEventListener('click', () => showDisasmTab('json'));
disasmTabText.addEventListener('click', () => showDisasmTab('text'));

document.getElementById('disasm-btn').addEventListener('click', async () => {
  const kind = disasmKind.value;
  const number = disasmNumber.value;
  try {
    const [lines, text] = await Promise.all([
      fetchJson(`/disasm?${kind}=${number}`),
      fetch(`/disasm-text?${kind}=${number}`).then((r) => r.text()),
    ]);
    disasmOutputJson.textContent = JSON.stringify(lines, null, 2);
    disasmOutputText.textContent = text;
  } catch (e) {
    setStatus(e.message, true);
  }
});
