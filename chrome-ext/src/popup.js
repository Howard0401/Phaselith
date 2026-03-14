// Popup UI logic — communicates with background service worker

const enableToggle = document.getElementById('enableToggle');
const fftPilotToggle = document.getElementById('fftPilotToggle');
const strengthSlider = document.getElementById('strength');
const hfSlider = document.getElementById('hfReconstruction');
const dynSlider = document.getElementById('dynamics');
const statusEl = document.getElementById('status');
const styleGrid = document.getElementById('styleGrid');
const styleBtns = styleGrid.querySelectorAll('.style-btn');

let currentStylePreset = 0;

// Load saved state
chrome.storage.local.get(['enabled', 'strength', 'hfReconstruction', 'dynamics', 'stylePreset', 'synthesisMode'], (data) => {
  enableToggle.checked = data.enabled ?? false;
  fftPilotToggle.checked = (data.synthesisMode ?? 0) === 1;
  strengthSlider.value = data.strength ?? 70;
  hfSlider.value = data.hfReconstruction ?? 80;
  dynSlider.value = data.dynamics ?? 60;
  currentStylePreset = data.stylePreset ?? 0;
  updateStyleUI();
  updateUI();
});

function updateUI() {
  document.getElementById('strengthValue').textContent = strengthSlider.value + '%';
  document.getElementById('hfValue').textContent = hfSlider.value + '%';
  document.getElementById('dynValue').textContent = dynSlider.value + '%';

  if (enableToggle.checked) {
    statusEl.textContent = 'Active';
    statusEl.className = 'status active';
  } else {
    statusEl.textContent = 'Inactive';
    statusEl.className = 'status inactive';
  }
}

function updateStyleUI() {
  styleBtns.forEach(btn => {
    const preset = parseInt(btn.dataset.preset);
    btn.classList.toggle('selected', preset === currentStylePreset);
  });
}

function saveAndNotify() {
  const state = {
    enabled: enableToggle.checked,
    strength: parseInt(strengthSlider.value),
    hfReconstruction: parseInt(hfSlider.value),
    dynamics: parseInt(dynSlider.value),
    stylePreset: currentStylePreset,
    synthesisMode: fftPilotToggle.checked ? 1 : 0,
  };
  chrome.storage.local.set(state);
  chrome.runtime.sendMessage({ type: 'CONFIG_UPDATE', ...state });
  updateUI();
}

enableToggle.addEventListener('change', async () => {
  // Save config FIRST so background reads the correct values
  const state = {
    enabled: enableToggle.checked,
    strength: parseInt(strengthSlider.value),
    hfReconstruction: parseInt(hfSlider.value),
    dynamics: parseInt(dynSlider.value),
    stylePreset: currentStylePreset,
    synthesisMode: fftPilotToggle.checked ? 1 : 0,
  };
  await chrome.storage.local.set(state);
  updateUI();

  if (enableToggle.checked) {
    chrome.runtime.sendMessage({ type: 'START_CAPTURE' });
  } else {
    chrome.runtime.sendMessage({ type: 'STOP_CAPTURE' });
  }
  // Also send CONFIG_UPDATE for immediate effect
  chrome.runtime.sendMessage({ type: 'CONFIG_UPDATE', ...state });
});

strengthSlider.addEventListener('input', saveAndNotify);
hfSlider.addEventListener('input', saveAndNotify);
dynSlider.addEventListener('input', saveAndNotify);
fftPilotToggle.addEventListener('change', saveAndNotify);

// Style preset buttons
styleBtns.forEach(btn => {
  btn.addEventListener('click', () => {
    currentStylePreset = parseInt(btn.dataset.preset);
    updateStyleUI();
    saveAndNotify();
  });
});
