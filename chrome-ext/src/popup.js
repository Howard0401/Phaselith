// Popup UI logic — communicates with background service worker

const enableToggle = document.getElementById('enableToggle');
const strengthSlider = document.getElementById('strength');
const hfSlider = document.getElementById('hfReconstruction');
const dynSlider = document.getElementById('dynamics');
const statusEl = document.getElementById('status');

// Load saved state
chrome.storage.local.get(['enabled', 'strength', 'hfReconstruction', 'dynamics'], (data) => {
  enableToggle.checked = data.enabled ?? false;
  strengthSlider.value = data.strength ?? 70;
  hfSlider.value = data.hfReconstruction ?? 80;
  dynSlider.value = data.dynamics ?? 60;
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

function saveAndNotify() {
  const state = {
    enabled: enableToggle.checked,
    strength: parseInt(strengthSlider.value),
    hfReconstruction: parseInt(hfSlider.value),
    dynamics: parseInt(dynSlider.value),
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
