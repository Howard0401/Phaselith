// Background service worker
// Manages tab capture and offscreen document lifecycle.
// Reads saved config from chrome.storage and sends to offscreen with stream.

let offscreenCreated = false;

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'START_CAPTURE') {
    startCapture();
  } else if (msg.type === 'STOP_CAPTURE') {
    chrome.runtime.sendMessage({ type: 'OFFSCREEN_STOP' }).catch(() => {});
  } else if (msg.type === 'CONFIG_UPDATE') {
    // Forward config to offscreen
    chrome.runtime.sendMessage(msg).catch(() => {});
  }
  // Never return true — avoids "message channel closed" errors
});

async function startCapture() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id) return;

    const streamId = await chrome.tabCapture.getMediaStreamId({
      targetTabId: tab.id,
    });

    await ensureOffscreen();

    // Read saved config from storage (offscreen can't access chrome.storage)
    const config = await chrome.storage.local.get(
      ['strength', 'hfReconstruction', 'dynamics', 'enabled']
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
  if (offscreenCreated) return;

  try {
    const existing = await chrome.offscreen.hasDocument();
    if (existing) {
      offscreenCreated = true;
      return;
    }
  } catch (e) {
    // hasDocument might not exist in older Chrome versions
  }

  await chrome.offscreen.createDocument({
    url: 'offscreen.html',
    reasons: ['USER_MEDIA'],
    justification: 'ASCE audio processing requires AudioContext with WASM AudioWorklet',
  });
  offscreenCreated = true;
}
