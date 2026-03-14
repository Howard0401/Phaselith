// Popup UI logic — communicates with background service worker

// ── i18n (manual language selector) ──
const I18N = {
  en: {
    statusActive: 'Active',
    statusInactive: 'Inactive',
    enableEnhancement: 'Enable Enhancement',
    styleLabel: 'STYLE',
    presetReference: 'Reference',
    presetGrand: 'Grand',
    presetSmooth: 'Smooth',
    presetVocal: 'Vocal',
    presetPunch: 'Punch',
    presetAir: 'Air',
    presetNight: 'Night',
    strengthLabel: 'Strength',
    hfLabel: 'HF Reconstruction',
    dynamicsLabel: 'Dynamics',
    footerVersion: 'v0.1.0',
  },
  'zh-TW': {
    statusActive: '\u5df2\u555f\u7528',
    statusInactive: '\u672a\u555f\u7528',
    enableEnhancement: '\u555f\u7528\u589e\u5f37',
    styleLabel: '\u98a8\u683c',
    presetReference: '\u53c3\u8003',
    presetGrand: '\u78c5\u7921',
    presetSmooth: '\u6eab\u6f64',
    presetVocal: '\u4eba\u8072',
    presetPunch: '\u529b\u9053',
    presetAir: '\u7a7a\u6c23\u611f',
    presetNight: '\u591c\u9593',
    strengthLabel: '\u5f37\u5ea6',
    hfLabel: '\u9ad8\u983b\u91cd\u5efa',
    dynamicsLabel: '\u52d5\u614b',
    footerVersion: 'v0.1.0',
  },
  'zh-CN': {
    statusActive: '\u5df2\u542f\u7528',
    statusInactive: '\u672a\u542f\u7528',
    enableEnhancement: '\u542f\u7528\u589e\u5f3a',
    styleLabel: '\u98ce\u683c',
    presetReference: '\u53c2\u8003',
    presetGrand: '\u78c5\u7904',
    presetSmooth: '\u6e29\u6da6',
    presetVocal: '\u4eba\u58f0',
    presetPunch: '\u529b\u5ea6',
    presetAir: '\u7a7a\u6c14\u611f',
    presetNight: '\u591c\u95f4',
    strengthLabel: '\u5f3a\u5ea6',
    hfLabel: '\u9ad8\u9891\u91cd\u5efa',
    dynamicsLabel: '\u52a8\u6001',
    footerVersion: 'v0.1.0',
  },
};

function detectLang() {
  // navigator.language returns e.g. "zh-TW", "zh-CN", "zh", "en-US", "en"
  const lang = navigator.language || 'en';
  if (lang.startsWith('zh')) {
    // zh-TW, zh-Hant → 繁中; zh-CN, zh-Hans, zh → 简中
    if (lang === 'zh-TW' || lang === 'zh-Hant' || lang.startsWith('zh-Hant')) return 'zh-TW';
    return 'zh-CN';
  }
  return 'en';
}

let currentLang = detectLang();

function t(key) {
  return (I18N[currentLang] || I18N.en)[key] || I18N.en[key] || key;
}

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach(el => {
    el.textContent = t(el.getAttribute('data-i18n'));
  });
  // Update status text to match current state
  updateUI();
  // Highlight active lang button
  document.querySelectorAll('.lang-btn').forEach(btn => {
    btn.classList.toggle('selected', btn.dataset.lang === currentLang);
  });
}

// ── DOM refs ──
const enableToggle = document.getElementById('enableToggle');
const strengthSlider = document.getElementById('strength');
const hfSlider = document.getElementById('hfReconstruction');
const dynSlider = document.getElementById('dynamics');
const statusEl = document.getElementById('status');
const styleGrid = document.getElementById('styleGrid');
const styleBtns = styleGrid.querySelectorAll('.style-btn');

let currentStylePreset = 0;

// FFT Pilot Mode is always on — no UI toggle needed
const SYNTHESIS_MODE = 1; // FftOlaPilot

// ── Load saved state ──
chrome.storage.local.get(['enabled', 'strength', 'hfReconstruction', 'dynamics', 'stylePreset', 'lang'], (data) => {
  enableToggle.checked = data.enabled ?? false;
  strengthSlider.value = data.strength ?? 70;
  hfSlider.value = data.hfReconstruction ?? 80;
  dynSlider.value = data.dynamics ?? 60;
  currentStylePreset = data.stylePreset ?? 0;
  currentLang = data.lang || detectLang();
  updateStyleUI();
  applyI18n();
});

function updateUI() {
  document.getElementById('strengthValue').textContent = strengthSlider.value + '%';
  document.getElementById('hfValue').textContent = hfSlider.value + '%';
  document.getElementById('dynValue').textContent = dynSlider.value + '%';

  if (enableToggle.checked) {
    statusEl.textContent = t('statusActive');
    statusEl.className = 'status-badge active';
  } else {
    statusEl.textContent = t('statusInactive');
    statusEl.className = 'status-badge inactive';
  }
}

function updateStyleUI() {
  styleBtns.forEach(btn => {
    const preset = parseInt(btn.dataset.preset);
    btn.classList.toggle('selected', preset === currentStylePreset);
  });
}

function buildState() {
  return {
    enabled: enableToggle.checked,
    strength: parseInt(strengthSlider.value),
    hfReconstruction: parseInt(hfSlider.value),
    dynamics: parseInt(dynSlider.value),
    stylePreset: currentStylePreset,
    synthesisMode: SYNTHESIS_MODE,
  };
}

function saveAndNotify() {
  const state = buildState();
  chrome.storage.local.set(state);
  chrome.runtime.sendMessage({ type: 'CONFIG_UPDATE', ...state });
  updateUI();
}

enableToggle.addEventListener('change', async () => {
  const state = buildState();
  await chrome.storage.local.set(state);
  updateUI();

  if (enableToggle.checked) {
    chrome.runtime.sendMessage({ type: 'START_CAPTURE' });
  } else {
    chrome.runtime.sendMessage({ type: 'STOP_CAPTURE' });
  }
  chrome.runtime.sendMessage({ type: 'CONFIG_UPDATE', ...state });
});

strengthSlider.addEventListener('input', saveAndNotify);
hfSlider.addEventListener('input', saveAndNotify);
dynSlider.addEventListener('input', saveAndNotify);

// Style preset buttons
styleBtns.forEach(btn => {
  btn.addEventListener('click', () => {
    currentStylePreset = parseInt(btn.dataset.preset);
    updateStyleUI();
    saveAndNotify();
  });
});

// Language switcher
document.querySelectorAll('.lang-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    currentLang = btn.dataset.lang;
    chrome.storage.local.set({ lang: currentLang });
    applyI18n();
  });
});
