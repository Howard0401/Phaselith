// Background service worker
// Manages tab capture and offscreen document lifecycle.
// Reads saved config from chrome.storage and sends to offscreen with stream.
//
// Tab-follow behavior: when enabled, automatically re-captures audio
// when the user switches to a different tab. This ensures the processing
// chain always follows the active tab without manual re-toggle.

let offscreenCreated = false;
let capturedTabId = null; // Currently captured tab
let isEnabled = false;    // Whether the extension is active
let captureGeneration = 0; // Monotonic counter to discard stale captures

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
    chrome.runtime.sendMessage(msg).catch(() => {});
  }
  // Never return true — avoids "message channel closed" errors
});

// Auto-follow active tab: when user switches tabs while enabled,
// re-capture the new tab's audio automatically.
chrome.tabs.onActivated.addListener(async (activeInfo) => {
  if (!isEnabled) return;
  if (activeInfo.tabId === capturedTabId) return; // same tab, no-op

  // Verify the tab is not an internal Chrome page.
  // Note: without "tabs" permission, tab.url is only available for tabs where
  // the extension has host permission. If tab.url is undefined, we still attempt
  // capture — tabCapture.getMediaStreamId will fail gracefully for uncapturable tabs.
  try {
    const tab = await chrome.tabs.get(activeInfo.tabId);
    const url = tab?.url || '';
    if (url.startsWith('chrome://') || url.startsWith('chrome-extension://')) {
      return; // skip internal pages
    }
  } catch {
    return; // tab doesn't exist
  }

  console.log(`ASCE: Tab switched ${capturedTabId} → ${activeInfo.tabId}, re-capturing`);
  startCapture();
});

// Handle captured tab being closed
chrome.tabs.onRemoved.addListener((tabId) => {
  if (tabId === capturedTabId) {
    console.log('ASCE: Captured tab closed, stopping');
    capturedTabId = null;
    chrome.runtime.sendMessage({ type: 'OFFSCREEN_STOP' }).catch(() => {});
    // Don't set isEnabled=false — if user opens a new tab it will auto-capture
  }
});

async function startCapture() {
  // Increment generation — any in-flight capture with an older generation
  // will be discarded when it resolves. This prevents fast tab-switching
  // from letting a stale capture overwrite a newer one.
  const thisGeneration = ++captureGeneration;

  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id) return;

    // Skip internal Chrome pages (can't capture them)
    if (tab.url?.startsWith('chrome://') || tab.url?.startsWith('chrome-extension://')) {
      return;
    }

    const streamId = await chrome.tabCapture.getMediaStreamId({
      targetTabId: tab.id,
    });

    // Race guard: if another startCapture() was called while we awaited,
    // this capture is stale — discard it.
    if (thisGeneration !== captureGeneration) {
      console.log(`ASCE: Discarding stale capture (gen ${thisGeneration}, current ${captureGeneration})`);
      return;
    }

    capturedTabId = tab.id;

    await ensureOffscreen();

    // Second race check after ensureOffscreen (also async)
    if (thisGeneration !== captureGeneration) {
      console.log(`ASCE: Discarding stale capture after offscreen (gen ${thisGeneration})`);
      return;
    }

    // Read saved config from storage (offscreen can't access chrome.storage)
    const config = await chrome.storage.local.get(
      ['strength', 'hfReconstruction', 'dynamics', 'enabled', 'stylePreset']
    );

    chrome.runtime.sendMessage({
      type: 'OFFSCREEN_START',
      streamId,
      tabId: tab.id,
      config,
    }).catch(() => {});
  } catch (err) {
    console.error('ASCE: Failed to start capture:', err);
  }
}

async function ensureOffscreen() {
  // Always check hasDocument() — Chrome can kill offscreen docs under resource
  // pressure, making the cached offscreenCreated flag stale.
  try {
    const existing = await chrome.offscreen.hasDocument();
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
    justification: 'ASCE audio processing requires AudioContext with WASM AudioWorklet',
  });
  offscreenCreated = true;
}
