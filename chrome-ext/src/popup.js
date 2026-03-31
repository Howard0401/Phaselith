// Popup UI logic — communicates with background service worker

// ── License API ──
const LICENSE_API = 'https://license.phaselith.com';
const REVALIDATE_DAYS = 7;

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
    transientLabel: 'Transient',
    paramPresetLabel: 'PARAMETER PRESET',
    paramPresetDefault: 'Default',
    paramPresetHoward: 'Howard',
    advancedLabel: 'Advanced \u25BC',
    advancedLabelOpen: 'Advanced \u25B2',
    toneLabel: 'TONE',
    warmthLabel: 'Warmth',
    airBrightnessLabel: 'Air / Brightness',
    smoothnessLabel: 'Smoothness',
    bodyLabel: 'Body',
    bodyPassLabel: 'Body Pass A/B',
    bodyPassHint: 'Windows only. Re-opens the 180-500 Hz body lane below cutoff for quick A/B.',
    refineLabel: 'REFINE A/B',
    refineHint: 'Windows only. 0% = current sound; raise each control to compare against the default path.',
    hfTameLabel: 'HF Tame',
    airContinuityLabel: 'Air Continuity',
    ambienceGlueLabel: 'Ambience Glue',
    levelCheckLabel: 'LIVE LEVEL CHECK',
    inputLevelLabel: 'Input RMS',
    outputLevelLabel: 'Output RMS',
    levelDeltaLabel: 'Delta',
    levelCheckHint: 'Short-term RMS only. If Delta stays near 0 dB, the Chrome output is not actually much quieter.',
    spatialLabel: 'SPATIAL',
    spatialSpreadLabel: 'Stereo Spread',
    ambienceLabel: 'Ambience Preserve',
    impactLabel: 'IMPACT',
    impactGainLabel: 'Impact Gain',
    footerVersion: 'v0.1.27',
    licenseLabel: 'License',
    activateBtn: 'Activate',
    previewBadge: 'Preview',
    previewHint: 'All features unlocked during preview. Feedback welcome!',
    freeHint: 'Free: Strength capped at 50%, Reference preset only',
    getProLink: 'View Plans \u2192',
    activating: 'Activating...',
    macRuntimeModeLabel: 'macOS Runtime Mode',
    macRuntimeModeBest: '1 (Best)',
    macRuntimeModeStable: '8 (Stable)',
    macRuntimeModeHint: 'macOS only. 1 sounds best but may crackle on some systems. 8 is the safer fallback.',
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
    transientLabel: '\u77ac\u614b',
    paramPresetLabel: '\u53c3\u6578\u9810\u8a2d',
    paramPresetDefault: '\u9810\u8a2d',
    paramPresetHoward: 'Howard',
    advancedLabel: '\u9032\u968e \u25BC',
    advancedLabelOpen: '\u9032\u968e \u25B2',
    toneLabel: '\u97f3\u8272',
    warmthLabel: '\u6eab\u6696\u611f',
    airBrightnessLabel: '\u7a7a\u6c23 / \u4eae\u5ea6',
    smoothnessLabel: '\u6ed1\u9806\u5ea6',
    bodyLabel: '\u539a\u5ea6',
    bodyPassLabel: '\u539a\u5ea6 A/B \u958b\u95dc',
    bodyPassHint: '\u50c5 Windows \u986f\u793a\u3002\u6703\u91cd\u65b0\u6253\u958b cutoff \u4ee5\u4e0b 180-500 Hz \u7684 body \u901a\u9053\uff0c\u65b9\u4fbf\u5feb\u901f A/B\u3002',
    refineLabel: '\u7d30\u4fee A/B',
    refineHint: '\u50c5 Windows \u986f\u793a\u30020% = \u76ee\u524d\u8072\u97f3\uff0c\u5f80\u4e0a\u63a8\u5c31\u80fd\u76f4\u63a5\u8ddf\u73fe\u884c\u8def\u5f91 A/B \u6bd4\u8f03\u3002',
    hfTameLabel: '\u9ad8\u983b\u6536\u6599',
    airContinuityLabel: '\u6c23\u97f3\u9023\u7e8c',
    ambienceGlueLabel: '\u5802\u97f3\u9ed8\u5408',
    levelCheckLabel: '\u5373\u6642\u97ff\u5ea6\u6aa2\u67e5',
    inputLevelLabel: '\u8f38\u5165 RMS',
    outputLevelLabel: '\u8f38\u51fa RMS',
    levelDeltaLabel: '\u5dee\u503c',
    levelCheckHint: '\u9019\u662f short-term RMS\uff0c\u4e0d\u662f LUFS\u3002\u5982\u679c Delta \u9577\u6642\u9593\u63a5\u8fd1 0 dB\uff0c\u5c31\u4e0d\u662f Chrome \u8f38\u51fa\u771f\u7684\u5c0f\u5f88\u591a\u3002',
    spatialLabel: '\u7a7a\u9593',
    spatialSpreadLabel: '\u7acb\u9ad4\u8076\u5bec\u5ea6',
    ambienceLabel: '\u6df7\u97ff\u4fdd\u7559',
    impactLabel: '\u885d\u64ca',
    impactGainLabel: '\u885d\u64ca\u589e\u76ca',
    footerVersion: 'v0.1.27',
    licenseLabel: '\u6388\u6b0a',
    activateBtn: '\u555f\u7528',
    previewBadge: '\u9810\u89bd\u7248',
    previewHint: '\u9810\u89bd\u671f\u9593\u5168\u529f\u80fd\u958b\u653e\uff0c\u6b61\u8fce\u56de\u994b\uff01',
    freeHint: '\u514d\u8cbb\u7248\uff1a\u5f37\u5ea6\u4e0a\u9650 50%\uff0c\u50c5\u53c3\u8003\u98a8\u683c',
    getProLink: '\u67e5\u770b\u65b9\u6848 \u2192',
    activating: '\u555f\u7528\u4e2d...',
    macRuntimeModeLabel: 'macOS \u57f7\u884c\u6a21\u5f0f',
    macRuntimeModeBest: '1 \uff08\u97f3\u8cea\u6700\u4f73\uff09',
    macRuntimeModeStable: '8 \uff08\u7a69\u5b9a\u512a\u5148\uff09',
    macRuntimeModeHint: '\u50c5 macOS \u986f\u793a\u30021 \u97f3\u8cea\u6700\u597d\uff0c\u4f46\u67d0\u4e9b\u7cfb\u7d71\u53ef\u80fd\u6703\u5361\u9813\uff1b8 \u662f\u8f03\u5b89\u5168\u7684\u5099\u63f4\u8a2d\u5b9a\u3002',
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
    transientLabel: '\u77ac\u6001',
    paramPresetLabel: '\u53c2\u6570\u9884\u8bbe',
    paramPresetDefault: '\u9884\u8bbe',
    paramPresetHoward: 'Howard',
    advancedLabel: '\u8fdb\u9636 \u25BC',
    advancedLabelOpen: '\u8fdb\u9636 \u25B2',
    toneLabel: '\u97f3\u8272',
    warmthLabel: '\u6e29\u6696\u611f',
    airBrightnessLabel: '\u7a7a\u6c14 / \u4eae\u5ea6',
    smoothnessLabel: '\u6ed1\u987a\u5ea6',
    bodyLabel: '\u539a\u5ea6',
    bodyPassLabel: '\u539a\u5ea6 A/B \u5f00\u5173',
    bodyPassHint: '\u4ec5 Windows \u663e\u793a\u3002\u4f1a\u91cd\u65b0\u6253\u5f00 cutoff \u4ee5\u4e0b 180-500 Hz \u7684 body \u901a\u9053\uff0c\u65b9\u4fbf\u5feb\u901f A/B\u3002',
    refineLabel: '\u7ec6\u4fee A/B',
    refineHint: '\u4ec5 Windows \u663e\u793a\u30020% = \u5f53\u524d\u58f0\u97f3\uff1b\u5f80\u4e0a\u63a8\u5c31\u80fd\u76f4\u63a5\u548c\u73b0\u884c\u8def\u5f84 A/B \u5bf9\u6bd4\u3002',
    hfTameLabel: '\u9ad8\u9891\u6536\u655b',
    airContinuityLabel: '\u6c14\u97f3\u8fde\u7eed',
    ambienceGlueLabel: '\u5802\u54cd\u9ed8\u5408',
    levelCheckLabel: '\u5b9e\u65f6\u54cd\u5ea6\u68c0\u67e5',
    inputLevelLabel: '\u8f93\u5165 RMS',
    outputLevelLabel: '\u8f93\u51fa RMS',
    levelDeltaLabel: '\u5dee\u503c',
    levelCheckHint: '\u8fd9\u662f short-term RMS\uff0c\u4e0d\u662f LUFS\u3002\u5982\u679c Delta \u957f\u65f6\u95f4\u63a5\u8fd1 0 dB\uff0c\u5c31\u4e0d\u662f Chrome \u8f93\u51fa\u771f\u7684\u53d8\u5c0f\u5f88\u591a\u3002',
    spatialLabel: '\u7a7a\u95f4',
    spatialSpreadLabel: '\u7acb\u4f53\u58f0\u5bbd\u5ea6',
    ambienceLabel: '\u6df7\u54cd\u4fdd\u7559',
    impactLabel: '\u51b2\u51fb',
    impactGainLabel: '\u51b2\u51fb\u589e\u76ca',
    footerVersion: 'v0.1.27',
    licenseLabel: '\u6388\u6743',
    activateBtn: '\u542f\u7528',
    previewBadge: '\u9884\u89c8\u7248',
    previewHint: '\u9884\u89c8\u671f\u95f4\u5168\u529f\u80fd\u5f00\u653e\uff0c\u6b22\u8fce\u53cd\u9988\uff01',
    freeHint: '\u514d\u8d39\u7248\uff1a\u5f3a\u5ea6\u4e0a\u9650 50%\uff0c\u4ec5\u53c2\u8003\u98ce\u683c',
    getProLink: '\u67e5\u770b\u65b9\u6848 \u2192',
    activating: '\u542f\u7528\u4e2d...',
    macRuntimeModeLabel: 'macOS \u8fd0\u884c\u6a21\u5f0f',
    macRuntimeModeBest: '1 \uff08\u97f3\u8d28\u6700\u4f73\uff09',
    macRuntimeModeStable: '8 \uff08\u7a33\u5b9a\u4f18\u5148\uff09',
    macRuntimeModeHint: '\u4ec5 macOS \u663e\u793a\u30021 \u97f3\u8d28\u6700\u597d\uff0c\u4f46\u5728\u67d0\u4e9b\u7cfb\u7edf\u4e0a\u53ef\u80fd\u4f1a\u5361\u987f\uff1b8 \u662f\u66f4\u4fdd\u5b88\u7684\u5907\u63f4\u8bbe\u5b9a\u3002',
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
const transientSlider = document.getElementById('transient');
const warmthSlider = document.getElementById('warmth');
const airBrightnessSlider = document.getElementById('airBrightness');
const smoothnessSlider = document.getElementById('smoothness');
const bodySlider = document.getElementById('bodyCtrl');
const bodyPassToggle = document.getElementById('bodyPassToggle');
const bodyPassSection = document.getElementById('bodyPassSection');
const hfTameSlider = document.getElementById('hfTame');
const airContinuitySlider = document.getElementById('airContinuity');
const ambienceGlueSlider = document.getElementById('ambienceGlue');
const spatialSpreadSlider = document.getElementById('spatialSpread');
const ambienceSlider = document.getElementById('ambiencePreserve');
const impactGainSlider = document.getElementById('impactGain');
const advancedToggle = document.getElementById('advancedToggle');
const advancedPanel = document.getElementById('advancedPanel');
const statusEl = document.getElementById('status');
const styleGrid = document.getElementById('styleGrid');
const styleBtns = styleGrid.querySelectorAll('.style-btn');
const macRuntimeSection = document.getElementById('macRuntimeSection');
const macSubBlockBtns = document.querySelectorAll('.mac-sub-block-btn');
const liveInputLevelEl = document.getElementById('liveInputLevel');
const liveOutputLevelEl = document.getElementById('liveOutputLevel');
const liveLevelDeltaEl = document.getElementById('liveLevelDelta');

let advancedOpen = false;

let currentStylePreset = 0;
let currentPlatformOs = 'unknown';
let currentMacSubBlock = 1;
let liveLevelState = null;
let liveLevelPollHandle = null;

chrome.runtime.getPlatformInfo((info) => {
  if (!chrome.runtime.lastError && info?.os) {
    currentPlatformOs = info.os;
    renderPlatformSections();
  }
});

function getSynthesisMode() {
  // Keep the original pilot path on Windows; use the safer legacy path on macOS
  // while we investigate the browser-runtime artifact there.
  return currentPlatformOs === 'mac' ? 0 : 1;
}

// ── Load saved state ──
chrome.storage.local.get(['enabled', 'strength', 'hfReconstruction', 'dynamics', 'transient', 'warmth', 'airBrightness', 'smoothness', 'bodyCtrl', 'bodyPassEnabled', 'hfTame', 'airContinuity', 'ambienceGlue', 'spatialSpread', 'ambiencePreserve', 'impactGain', 'stylePreset', 'lang', 'macSubBlockFrames', 'advancedOpen'], (data) => {
  enableToggle.checked = data.enabled ?? false;
  strengthSlider.value = data.strength ?? 70;
  hfSlider.value = data.hfReconstruction ?? 80;
  dynSlider.value = data.dynamics ?? 60;
  transientSlider.value = data.transient ?? 50;
  warmthSlider.value = data.warmth ?? 15;
  airBrightnessSlider.value = data.airBrightness ?? 50;
  smoothnessSlider.value = data.smoothness ?? 40;
  bodySlider.value = data.bodyCtrl ?? 40;
  bodyPassToggle.checked = data.bodyPassEnabled ?? false;
  hfTameSlider.value = data.hfTame ?? 0;
  airContinuitySlider.value = data.airContinuity ?? 0;
  ambienceGlueSlider.value = data.ambienceGlue ?? 0;
  spatialSpreadSlider.value = data.spatialSpread ?? 30;
  ambienceSlider.value = data.ambiencePreserve ?? 0;
  impactGainSlider.value = data.impactGain ?? 15;
  currentStylePreset = data.stylePreset ?? 0;
  currentLang = data.lang || detectLang();
  currentMacSubBlock = data.macSubBlockFrames ?? 1;
  advancedOpen = data.advancedOpen ?? false;
  if (advancedOpen) {
    advancedPanel.style.display = 'flex';
    advancedPanel.style.flexDirection = 'column';
    advancedPanel.style.gap = '12px';
  }
  updateStyleUI();
  updateMacSubBlockUI();
  applyI18n();
  renderPlatformSections();
  refreshLiveLevels();
});

function updateUI() {
  document.getElementById('strengthValue').textContent = strengthSlider.value + '%';
  document.getElementById('hfValue').textContent = hfSlider.value + '%';
  document.getElementById('dynValue').textContent = dynSlider.value + '%';
  document.getElementById('transientValue').textContent = transientSlider.value + '%';
  document.getElementById('warmthValue').textContent = warmthSlider.value + '%';
  document.getElementById('airBrightnessValue').textContent = airBrightnessSlider.value + '%';
  document.getElementById('smoothnessValue').textContent = smoothnessSlider.value + '%';
  document.getElementById('bodyValue').textContent = bodySlider.value + '%';
  document.getElementById('hfTameValue').textContent = hfTameSlider.value + '%';
  document.getElementById('airContinuityValue').textContent = airContinuitySlider.value + '%';
  document.getElementById('ambienceGlueValue').textContent = ambienceGlueSlider.value + '%';
  document.getElementById('spatialSpreadValue').textContent = spatialSpreadSlider.value + '%';
  document.getElementById('ambienceValue').textContent = ambienceSlider.value + '%';
  document.getElementById('impactGainValue').textContent = impactGainSlider.value + '%';

  // Advanced toggle label
  advancedToggle.textContent = advancedOpen ? t('advancedLabelOpen') : t('advancedLabel');
  updateLiveLevelUI();

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

function formatDb(value) {
  if (!Number.isFinite(value)) return '--';
  return `${value.toFixed(1)} dB`;
}

function formatDeltaDb(value) {
  if (!Number.isFinite(value)) return '--';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(2)} dB`;
}

function updateLiveLevelUI() {
  if (!liveInputLevelEl || !liveOutputLevelEl || !liveLevelDeltaEl) return;

  if (!liveLevelState || !enableToggle.checked) {
    liveInputLevelEl.textContent = '--';
    liveOutputLevelEl.textContent = '--';
    liveLevelDeltaEl.textContent = '--';
    return;
  }

  if (liveLevelState.updatedAt && Date.now() - liveLevelState.updatedAt > 1500) {
    liveInputLevelEl.textContent = '--';
    liveOutputLevelEl.textContent = '--';
    liveLevelDeltaEl.textContent = '--';
    return;
  }

  liveInputLevelEl.textContent = formatDb(liveLevelState.inputDbfs);
  liveOutputLevelEl.textContent = formatDb(liveLevelState.outputDbfs);
  liveLevelDeltaEl.textContent = formatDeltaDb(liveLevelState.deltaDb);
}

async function refreshLiveLevels() {
  try {
    const response = await chrome.runtime.sendMessage({ type: 'GET_LIVE_LEVELS' });
    if (response?.enabled === false) {
      liveLevelState = null;
    } else if (response?.payload) {
      liveLevelState = response.payload;
    }
    updateLiveLevelUI();
  } catch {
    // Ignore popup/background races during extension reloads.
  }
}

function ensureLiveLevelPolling() {
  if (liveLevelPollHandle !== null) {
    clearInterval(liveLevelPollHandle);
  }
  liveLevelPollHandle = setInterval(refreshLiveLevels, 250);
}

function updateMacSubBlockUI() {
  macSubBlockBtns.forEach(btn => {
    const subBlock = parseInt(btn.dataset.subBlock, 10);
    btn.classList.toggle('selected', subBlock === currentMacSubBlock);
  });
}

function renderPlatformSections() {
  document.querySelectorAll('.mac-only').forEach((el) => {
    el.style.display = currentPlatformOs === 'mac' ? 'block' : 'none';
  });
  document.querySelectorAll('.windows-only').forEach((el) => {
    el.style.display = currentPlatformOs === 'win' ? 'block' : 'none';
  });
}

function buildState() {
  return {
    enabled: enableToggle.checked,
    strength: parseInt(strengthSlider.value),
    hfReconstruction: parseInt(hfSlider.value),
    dynamics: parseInt(dynSlider.value),
    transient: parseInt(transientSlider.value),
    warmth: parseInt(warmthSlider.value),
    airBrightness: parseInt(airBrightnessSlider.value),
    smoothness: parseInt(smoothnessSlider.value),
    bodyCtrl: parseInt(bodySlider.value),
    bodyPassEnabled: currentPlatformOs === 'win' ? bodyPassToggle.checked : false,
    hfTame: currentPlatformOs === 'win' ? parseInt(hfTameSlider.value) : 0,
    airContinuity: currentPlatformOs === 'win' ? parseInt(airContinuitySlider.value) : 0,
    ambienceGlue: currentPlatformOs === 'win' ? parseInt(ambienceGlueSlider.value) : 0,
    spatialSpread: parseInt(spatialSpreadSlider.value),
    ambiencePreserve: parseInt(ambienceSlider.value),
    impactGain: parseInt(impactGainSlider.value),
    stylePreset: currentStylePreset,
    synthesisMode: getSynthesisMode(),
    macSubBlockFrames: currentPlatformOs === 'mac' ? currentMacSubBlock : undefined,
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
transientSlider.addEventListener('input', saveAndNotify);
warmthSlider.addEventListener('input', saveAndNotify);
airBrightnessSlider.addEventListener('input', saveAndNotify);
smoothnessSlider.addEventListener('input', saveAndNotify);
bodySlider.addEventListener('input', saveAndNotify);
hfTameSlider.addEventListener('input', saveAndNotify);
airContinuitySlider.addEventListener('input', saveAndNotify);
ambienceGlueSlider.addEventListener('input', saveAndNotify);
spatialSpreadSlider.addEventListener('input', saveAndNotify);
ambienceSlider.addEventListener('input', saveAndNotify);
impactGainSlider.addEventListener('input', saveAndNotify);
bodyPassToggle.addEventListener('change', saveAndNotify);

// Parameter presets
const PARAM_PRESETS = {
  default: { strength: 100, hfReconstruction: 100, dynamics: 100, transient: 50, warmth: 15, airBrightness: 50, smoothness: 40, bodyCtrl: 40, hfTame: 0, airContinuity: 0, ambienceGlue: 0, spatialSpread: 30, ambiencePreserve: 0, impactGain: 15 },
  howard:  { strength: 100, hfReconstruction: 100, dynamics: 100, transient: 100, warmth: 78, airBrightness: 46, smoothness: 87, bodyCtrl: 100, hfTame: 0, airContinuity: 0, ambienceGlue: 0, spatialSpread: 100, ambiencePreserve: 100, impactGain: 100 },
};

function applyParamPreset(name) {
  const p = PARAM_PRESETS[name];
  if (!p) return;
  strengthSlider.value = p.strength;
  hfSlider.value = p.hfReconstruction;
  dynSlider.value = p.dynamics;
  transientSlider.value = p.transient;
  warmthSlider.value = p.warmth;
  airBrightnessSlider.value = p.airBrightness;
  smoothnessSlider.value = p.smoothness;
  bodySlider.value = p.bodyCtrl;
  hfTameSlider.value = p.hfTame;
  airContinuitySlider.value = p.airContinuity;
  ambienceGlueSlider.value = p.ambienceGlue;
  spatialSpreadSlider.value = p.spatialSpread;
  ambienceSlider.value = p.ambiencePreserve;
  impactGainSlider.value = p.impactGain;
  saveAndNotify();
}

document.getElementById('presetDefault').addEventListener('click', () => applyParamPreset('default'));
document.getElementById('presetHoward').addEventListener('click', () => applyParamPreset('howard'));

advancedToggle.addEventListener('click', () => {
  advancedOpen = !advancedOpen;
  advancedPanel.style.display = advancedOpen ? 'flex' : 'none';
  advancedPanel.style.flexDirection = 'column';
  advancedPanel.style.gap = '12px';
  chrome.storage.local.set({ advancedOpen });
  updateUI();
});

async function saveMacSubBlockAndRestartIfNeeded(nextSubBlock) {
  currentMacSubBlock = nextSubBlock;
  updateMacSubBlockUI();
  await chrome.storage.local.set({ macSubBlockFrames: currentMacSubBlock });

  if (!enableToggle.checked) {
    return;
  }

  await chrome.runtime.sendMessage({ type: 'STOP_CAPTURE' });
  await chrome.runtime.sendMessage({ type: 'START_CAPTURE' });
}

macSubBlockBtns.forEach((btn) => {
  btn.addEventListener('click', async () => {
    if (currentPlatformOs !== 'mac') return;
    const nextSubBlock = parseInt(btn.dataset.subBlock, 10);
    if (nextSubBlock === currentMacSubBlock) return;
    await saveMacSubBlockAndRestartIfNeeded(nextSubBlock);
  });
});

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

chrome.runtime.onMessage.addListener((msg) => {
  if (msg.type === 'LIVE_LEVELS' && msg.payload) {
    liveLevelState = msg.payload;
    updateLiveLevelUI();
  }
});

ensureLiveLevelPolling();

// ── License Management ──

let licenseTier = 'Free'; // 'Free' or 'Pro'

async function getDeviceId() {
  const data = await chrome.storage.local.get('deviceId');
  if (data.deviceId) return data.deviceId;
  const id = crypto.randomUUID();
  await chrome.storage.local.set({ deviceId: id });
  return id;
}

async function loadLicense() {
  const data = await chrome.storage.local.get('license');
  const license = data.license;
  if (!license) {
    setTier('Free');
    return;
  }

  // Check if cached license is still valid
  const now = Date.now();
  const expiresAt = new Date(license.expires_at).getTime();
  if (expiresAt <= now) {
    // Expired — clear and go Free
    await chrome.storage.local.remove('license');
    setTier('Free');
    return;
  }

  // Use cached tier
  setTier('Pro', license.license_key);

  // Background revalidate if older than 7 days
  const validatedAt = license.validated_at || 0;
  if (now - validatedAt > REVALIDATE_DAYS * 86400000) {
    revalidateInBackground(license.license_key, license.device_id);
  }
}

async function revalidateInBackground(key, deviceId) {
  try {
    const res = await fetch(`${LICENSE_API}/license/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ license_key: key, device_id: deviceId }),
    });
    const data = await res.json();
    if (data.valid) {
      await chrome.storage.local.set({
        license: { license_key: key, device_id: deviceId, tier: 'Pro', expires_at: data.expires_at, validated_at: Date.now() },
      });
    } else {
      // Server says invalid — downgrade
      await chrome.storage.local.remove('license');
      setTier('Free');
    }
  } catch {
    // Network error — keep cached tier, don't lock out
  }
}

function setTier(tier, key) {
  licenseTier = tier;
  const activatedEl = document.getElementById('licenseActivated');
  const notActivatedEl = document.getElementById('licenseNotActivated');

  if (tier === 'Pro' && key) {
    activatedEl.style.display = 'block';
    notActivatedEl.style.display = 'none';
    document.getElementById('licenseKeyDisplay').textContent = key.substring(0, 11) + '...' + key.substring(key.length - 4);
    // Show expiry
    chrome.storage.local.get('license', (data) => {
      if (data.license?.expires_at) {
        const d = new Date(data.license.expires_at);
        document.getElementById('licenseExpiry').textContent = '→ ' + d.toLocaleDateString();
      }
    });
  } else {
    activatedEl.style.display = 'none';
    notActivatedEl.style.display = 'block';
  }

  enforceLimits();
}

function enforceLimits() {
  // ── Preview period: all features unlocked ──
  // TODO: Re-enable limits when ready to charge
  // const isFree = licenseTier !== 'Pro';
  // if (isFree && parseInt(strengthSlider.value) > 50) {
  //   strengthSlider.value = 50;
  //   saveAndNotify();
  // }
  // strengthSlider.max = isFree ? '50' : '100';
  // styleBtns.forEach(btn => {
  //   const preset = parseInt(btn.dataset.preset);
  //   if (isFree && preset !== 0) {
  //     btn.style.opacity = '0.3';
  //     btn.style.pointerEvents = 'none';
  //   } else {
  //     btn.style.opacity = '';
  //     btn.style.pointerEvents = '';
  //   }
  // });
  // if (isFree && currentStylePreset !== 0) {
  //   currentStylePreset = 0;
  //   updateStyleUI();
  //   saveAndNotify();
  // }
  strengthSlider.max = '100';
  document.getElementById('strengthValue').textContent = strengthSlider.value + '%';
}

// TODO: Re-enable license activation when ready to charge
/* Activate button
document.getElementById('activateBtn').addEventListener('click', async () => {
  const keyInput = document.getElementById('licenseKeyInput');
  const errorEl = document.getElementById('licenseError');
  const key = keyInput.value.trim();

  if (!key) return;

  // Validate key format client-side
  if (!/^PHSL-BR-[A-Za-z0-9]{24}$/.test(key)) {
    errorEl.textContent = 'Invalid key format';
    errorEl.style.display = 'block';
    return;
  }

  const activateBtn = document.getElementById('activateBtn');
  activateBtn.textContent = t('activating');
  activateBtn.disabled = true;
  errorEl.style.display = 'none';

  try {
    const deviceId = await getDeviceId();
    const res = await fetch(`${LICENSE_API}/license/activate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        license_key: key,
        device_id: deviceId,
        device_name: 'Chrome Extension',
        platform: 'chrome',
      }),
    });
    const data = await res.json();

    if (data.valid) {
      await chrome.storage.local.set({
        license: { license_key: key, device_id: deviceId, tier: 'Pro', expires_at: data.expires_at, validated_at: Date.now() },
      });
      setTier('Pro', key);
    } else {
      errorEl.textContent = data.error || 'Activation failed';
      errorEl.style.display = 'block';
    }
  } catch (e) {
    errorEl.textContent = 'Network error — check connection';
    errorEl.style.display = 'block';
  } finally {
    activateBtn.textContent = t('activateBtn');
    activateBtn.disabled = false;
  }
});

*/

// Load license on startup — skipped during preview
// loadLicense();
