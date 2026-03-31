// Background service worker
// Manages tab capture and offscreen document lifecycle.
// Reads saved config from chrome.storage and sends to offscreen with stream.
//
// Capture lifecycle is user-gesture bound: the extension can capture the
// currently focused tab when the user invokes it, but Chrome requires a
// fresh click before capturing a different tab.

let offscreenCreated = false;
let capturedTabId = null; // Currently captured tab
let isEnabled = false;    // Whether the extension is active
let captureGeneration = 0; // Monotonic counter to discard stale captures
let lastGestureNoticeTabId = null; // Avoid spamming the same tab-switch notice
let platformOs = 'unknown';
const DEBUG_VERSION = 'mac-transient-safe-1';
const DEFAULT_MAC_TRANSIENT_MODE = 'transient-safe';

function normalizeMacTransientMode(mode) {
  switch (mode) {
    case 'hop-probe':
      return 'transient-safe';
    case 'declip-probe':
      return 'declip-safe';
    default:
      return mode;
  }
}

chrome.runtime.getPlatformInfo((info) => {
  if (chrome.runtime.lastError) {
    console.warn('Phaselith: Failed to get platform info:', chrome.runtime.lastError.message);
    return;
  }
  platformOs = info?.os || 'unknown';
  console.log(`Phaselith: Platform detected (${platformOs}) [${DEBUG_VERSION}]`);
});

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'START_CAPTURE') {
    isEnabled = true;
    startCapture();
  } else if (msg.type === 'STOP_CAPTURE') {
    isEnabled = false;
    capturedTabId = null;
    chrome.runtime.sendMessage({ type: 'OFFSCREEN_STOP' }).catch(() => {});
  } else if (msg.type === 'CONFIG_UPDATE') {
    // Forward config to offscreen
    chrome.runtime.sendMessage({ ...msg, platformOs }).catch(() => {});
  } else if (msg.type === 'DEBUG_EVENT') {
    console.log('Phaselith DEBUG:', JSON.stringify(msg.payload));
  }
  // Never return true — avoids "message channel closed" errors
});

// Chrome's tabCapture permission model requires a fresh user invocation
// for a newly active tab. We therefore keep the current capture running
// and ask the user to re-invoke the extension on the new tab instead of
// attempting an automatic re-capture that Chrome will reject.
chrome.tabs.onActivated.addListener(async (activeInfo) => {
  if (!isEnabled) return;
  if (activeInfo.tabId === capturedTabId) return; // same tab, no-op

  if (!await isCapturableTab(activeInfo.tabId)) return;

  if (lastGestureNoticeTabId === activeInfo.tabId) return;
  lastGestureNoticeTabId = activeInfo.tabId;

  console.log(
    `Phaselith: Tab switched ${capturedTabId} → ${activeInfo.tabId}. ` +
    'Chrome requires a fresh extension click to capture the new tab, so auto-follow is skipped.'
  );
});

// Handle captured tab being closed
chrome.tabs.onRemoved.addListener((tabId) => {
  if (tabId === capturedTabId) {
    console.log('Phaselith: Captured tab closed, stopping');
    capturedTabId = null;
    chrome.runtime.sendMessage({ type: 'OFFSCREEN_STOP' }).catch(() => {});
    // Don't set isEnabled=false — user can re-arm capture on another tab
    // with a fresh extension click.
  }
});

// Check if a tab can be captured (not a restricted page)
async function isCapturableTab(tabId) {
  try {
    const tab = await chrome.tabs.get(tabId);
    const url = tab?.url || '';

    // Without "tabs" permission, tab.url may be undefined for pages
    // where we don't have host permission. In that case, we still
    // attempt capture — it will fail gracefully.
    if (!url) return true;

    // Chrome internal pages, devtools, web store, and new tab cannot be captured
    if (url.startsWith('chrome://') ||
        url.startsWith('chrome-extension://') ||
        url.startsWith('devtools://') ||
        url.startsWith('chrome-devtools://') ||
        url.startsWith('https://chrome.google.com/webstore') ||
        url.startsWith('https://chromewebstore.google.com') ||
        url.startsWith('about:') ||
        url.startsWith('edge://') ||
        url === '') {
      return false;
    }
    return true;
  } catch {
    return false; // tab doesn't exist
  }
}

async function startCapture() {
  // Increment generation — any in-flight capture with an older generation
  // will be discarded when it resolves. This prevents fast tab-switching
  // from letting a stale capture overwrite a newer one.
  const thisGeneration = ++captureGeneration;

  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id) return;

    if (!await isCapturableTab(tab.id)) {
      console.log('Phaselith: Skipping uncapturable tab:', tab.url || '(no url)');
      return;
    }

    let streamId;
    try {
      streamId = await chrome.tabCapture.getMediaStreamId({
        targetTabId: tab.id,
      });
    } catch (captureErr) {
      // tabCapture follows the same user-invocation model as activeTab.
      // If the user switched tabs after enabling the extension, Chrome will
      // reject a new capture until they click the extension again on that tab.
      console.log(
        'Phaselith: Cannot capture tab without a fresh user gesture. ' +
        'Click the extension while focused on the target tab.',
        tab.id,
        captureErr?.message || captureErr
      );
      return;
    }

    // Race guard: if another startCapture() was called while we awaited,
    // this capture is stale — discard it.
    if (thisGeneration !== captureGeneration) {
      console.log(`Phaselith: Discarding stale capture (gen ${thisGeneration}, current ${captureGeneration})`);
      return;
    }

    capturedTabId = tab.id;

    await ensureOffscreen(platformOs === 'mac');

    // Second race check after ensureOffscreen (also async)
    if (thisGeneration !== captureGeneration) {
      console.log(`Phaselith: Discarding stale capture after offscreen (gen ${thisGeneration})`);
      return;
    }

    // Read saved config from storage (offscreen can't access chrome.storage)
    const config = await chrome.storage.local.get(
      ['strength', 'hfReconstruction', 'dynamics', 'transient', 'warmth', 'airBrightness', 'smoothness', 'bodyCtrl', 'bodyPassEnabled', 'spatialSpread', 'ambiencePreserve', 'impactGain', 'enabled', 'stylePreset', 'synthesisMode', 'macTransientMode', 'macSubBlockFrames']
    );
    config.macTransientMode = normalizeMacTransientMode(config.macTransientMode);
    if (platformOs === 'mac' && (!config.macTransientMode || config.macTransientMode === 'declip-safe')) {
      config.macTransientMode = DEFAULT_MAC_TRANSIENT_MODE;
      await chrome.storage.local.set({ macTransientMode: config.macTransientMode });
    } else if (!config.macTransientMode) {
      config.macTransientMode = DEFAULT_MAC_TRANSIENT_MODE;
    }
    if (platformOs === 'mac') {
      config.macSubBlockFrames = config.macSubBlockFrames === 8 ? 8 : 1;
    }

    chrome.runtime.sendMessage({
      type: 'OFFSCREEN_START',
      streamId,
      tabId: tab.id,
      config,
      platformOs,
      debugVersion: DEBUG_VERSION,
    }).catch(() => {});
  } catch (err) {
    console.error('Phaselith: Failed to start capture:', err);
  }
}

async function ensureOffscreen(forceRecreate = false) {
  // Always check hasDocument() — Chrome can kill offscreen docs under resource
  // pressure, making the cached offscreenCreated flag stale.
  try {
    let existing = await chrome.offscreen.hasDocument();
    if (existing && forceRecreate) {
      await chrome.offscreen.closeDocument();
      offscreenCreated = false;
      existing = await chrome.offscreen.hasDocument();
    }
    if (existing) {
      offscreenCreated = true;
      return;
    }
  } catch (e) {
    // hasDocument might not exist in older Chrome versions — fall back to flag
    if (offscreenCreated) return;
  }

  offscreenCreated = false; // reset in case it was stale
  await chrome.offscreen.createDocument({
    url: 'offscreen.html',
    reasons: ['USER_MEDIA'],
    justification: 'Phaselith audio processing requires AudioContext with WASM AudioWorklet',
  });
  offscreenCreated = true;
}
