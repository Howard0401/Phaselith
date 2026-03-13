// Background service worker
// Manages tab capture and offscreen document lifecycle.
//
// Flow: tabCapture.getMediaStreamId → offscreen document → AudioContext + WASM

let offscreenCreated = false;

// Handle messages from popup and offscreen
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'START_CAPTURE') {
    startCapture();
  } else if (msg.type === 'STOP_CAPTURE') {
    stopCapture();
  } else if (msg.type === 'CONFIG_UPDATE') {
    // Forward config to offscreen document
    forwardToOffscreen(msg);
  }
  return true;
});

async function startCapture() {
  try {
    // Get the active tab
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab?.id) return;

    // Get media stream ID for this tab
    const streamId = await chrome.tabCapture.getMediaStreamId({
      targetTabId: tab.id,
    });

    // Create offscreen document if not exists
    await ensureOffscreen();

    // Send stream ID to offscreen document to start processing
    chrome.runtime.sendMessage({
      type: 'OFFSCREEN_START',
      streamId,
      tabId: tab.id,
    });
  } catch (err) {
    console.error('ASCE: Failed to start capture:', err);
  }
}

async function stopCapture() {
  chrome.runtime.sendMessage({ type: 'OFFSCREEN_STOP' });
}

async function ensureOffscreen() {
  if (offscreenCreated) return;

  const existing = await chrome.offscreen.hasDocument?.() ?? false;
  if (existing) {
    offscreenCreated = true;
    return;
  }

  await chrome.offscreen.createDocument({
    url: 'offscreen.html',
    reasons: ['USER_MEDIA'],
    justification: 'ASCE audio processing requires AudioContext with WASM AudioWorklet',
  });
  offscreenCreated = true;
}

function forwardToOffscreen(msg) {
  chrome.runtime.sendMessage(msg).catch(() => {
    // Offscreen might not be created yet, ignore
  });
}
