// Popup UI logic — communicates with background service worker

// ── License API ──
const LICENSE_API = 'http://localhost:8787'; // dev; production: https://license.phaselith.com
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
    footerVersion: 'v0.1.25',
    licenseLabel: 'License',
    activateBtn: 'Activate',
    freeHint: 'Free: Strength capped at 50%, Reference preset only',
    getProLink: 'Upgrade to Pro \u2192',
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
    footerVersion: 'v0.1.25',
    licenseLabel: '\u6388\u6b0a',
    activateBtn: '\u555f\u7528',
    freeHint: '\u514d\u8cbb\u7248\uff1a\u5f37\u5ea6\u4e0a\u9650 50%\uff0c\u50c5\u53c3\u8003\u98a8\u683c',
    getProLink: '\u5347\u7d1a\u5c08\u696d\u7248 \u2192',
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
    footerVersion: 'v0.1.25',
    licenseLabel: '\u6388\u6743',
    activateBtn: '\u542f\u7528',
    freeHint: '\u514d\u8d39\u7248\uff1a\u5f3a\u5ea6\u4e0a\u9650 50%\uff0c\u4ec5\u53c2\u8003\u98ce\u683c',
    getProLink: '\u5347\u7ea7\u4e13\u4e1a\u7248 \u2192',
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
const statusEl = document.getElementById('status');
const styleGrid = document.getElementById('styleGrid');
const styleBtns = styleGrid.querySelectorAll('.style-btn');
const macRuntimeSection = document.getElementById('macRuntimeSection');
const macSubBlockBtns = document.querySelectorAll('.mac-sub-block-btn');

let currentStylePreset = 0;
let currentPlatformOs = 'unknown';
let currentMacSubBlock = 1;

chrome.runtime.getPlatformInfo((info) => {
  if (!chrome.runtime.lastError && info?.os) {
    currentPlatformOs = info.os;
    renderMacRuntimeSection();
  }
});

function getSynthesisMode() {
  // Keep the original pilot path on Windows; use the safer legacy path on macOS
  // while we investigate the browser-runtime artifact there.
  return currentPlatformOs === 'mac' ? 0 : 1;
}

// ── Load saved state ──
chrome.storage.local.get(['enabled', 'strength', 'hfReconstruction', 'dynamics', 'stylePreset', 'lang', 'macSubBlockFrames'], (data) => {
  enableToggle.checked = data.enabled ?? false;
  strengthSlider.value = data.strength ?? 70;
  hfSlider.value = data.hfReconstruction ?? 80;
  dynSlider.value = data.dynamics ?? 60;
  currentStylePreset = data.stylePreset ?? 0;
  currentLang = data.lang || detectLang();
  currentMacSubBlock = data.macSubBlockFrames ?? 1;
  updateStyleUI();
  updateMacSubBlockUI();
  applyI18n();
  renderMacRuntimeSection();
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

function updateMacSubBlockUI() {
  macSubBlockBtns.forEach(btn => {
    const subBlock = parseInt(btn.dataset.subBlock, 10);
    btn.classList.toggle('selected', subBlock === currentMacSubBlock);
  });
}

function renderMacRuntimeSection() {
  macRuntimeSection.style.display = currentPlatformOs === 'mac' ? 'block' : 'none';
}

function buildState() {
  return {
    enabled: enableToggle.checked,
    strength: parseInt(strengthSlider.value),
    hfReconstruction: parseInt(hfSlider.value),
    dynamics: parseInt(dynSlider.value),
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
  const isFree = licenseTier !== 'Pro';

  // Strength cap
  if (isFree && parseInt(strengthSlider.value) > 50) {
    strengthSlider.value = 50;
    saveAndNotify();
  }
  strengthSlider.max = isFree ? '50' : '100';
  document.getElementById('strengthValue').textContent = strengthSlider.value + '%';

  // Preset lock — only Reference for Free
  styleBtns.forEach(btn => {
    const preset = parseInt(btn.dataset.preset);
    if (isFree && preset !== 0) {
      btn.style.opacity = '0.3';
      btn.style.pointerEvents = 'none';
    } else {
      btn.style.opacity = '';
      btn.style.pointerEvents = '';
    }
  });

  // Force Reference if Free and non-Reference selected
  if (isFree && currentStylePreset !== 0) {
    currentStylePreset = 0;
    updateStyleUI();
    saveAndNotify();
  }
}

// Activate button
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

// Load license on startup
loadLicense();
