// Offscreen document: hosts AudioContext + WASM AudioWorklet
//
// 1. Fetches WASM binary and sends to worklet (worklet can't fetch)
// 2. Does NOT use chrome.storage (not available in offscreen)
// 3. Config comes via msg.config from background

let audioCtx = null;
let sourceNode = null;
let workletNode = null;
let mediaStream = null;
let wasmBinary = null; // Cached WASM binary
let capturedTabId = null; // Tab being processed

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'OFFSCREEN_START') {
    capturedTabId = msg.tabId ?? null;
    startProcessing(msg.streamId, msg.config);
  } else if (msg.type === 'OFFSCREEN_STOP') {
    capturedTabId = null;
    stopProcessing();
  } else if (msg.type === 'CONFIG_UPDATE') {
    updateConfig(msg);
  }
});

async function loadWasmBinary() {
  if (wasmBinary) return wasmBinary;
  const url = chrome.runtime.getURL('asce_wasm_bridge.wasm');
  const response = await fetch(url);
  wasmBinary = await response.arrayBuffer();
  return wasmBinary;
}

async function startProcessing(streamId, config) {
  try {
    stopProcessing();

    // Load WASM binary first (offscreen has fetch, worklet doesn't)
    const binary = await loadWasmBinary();

    mediaStream = await navigator.mediaDevices.getUserMedia({
      audio: {
        mandatory: {
          chromeMediaSource: 'tab',
          chromeMediaSourceId: streamId,
        },
      },
    });

    audioCtx = new AudioContext({ sampleRate: 48000 });
    await audioCtx.audioWorklet.addModule(chrome.runtime.getURL('src/worklet-processor.js'));

    sourceNode = audioCtx.createMediaStreamSource(mediaStream);
    workletNode = new AudioWorkletNode(audioCtx, 'asce-processor', {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [2],
    });

    // Listen for worklet messages
    workletNode.port.onmessage = (e) => {
      const d = e.data;
      if (d.type === 'WASM_READY') {
        console.log('ASCE: WASM engine ready');
        // Worklet caches and replays config on init — no need to resend here.
        // Sending startup config would overwrite newer CONFIG_UPDATE values
        // that arrived while WASM was loading.
      } else if (d.type === 'WASM_ERROR') {
        console.error('ASCE: WASM init error:', d.error);
      }
    };

    // Send initial config to worklet so it caches the values before WASM loads.
    // The worklet will replay these cached values in _initWasm() after WASM_READY.
    if (config) updateConfig(config);

    // Send WASM binary to worklet (it can't fetch on its own)
    // .slice(0) creates a copy so the cached wasmBinary stays intact
    const wasmCopy = binary.slice(0);
    workletNode.port.postMessage(
      { type: 'WASM_BINARY', buffer: wasmCopy },
      [wasmCopy] // transfer (zero-copy move to worklet thread)
    );

    sourceNode.connect(workletNode);
    workletNode.connect(audioCtx.destination);

    console.log(`ASCE: Audio processing started (tab ${capturedTabId})`);
  } catch (err) {
    console.error('ASCE: Failed to start processing:', err);
  }
}

function stopProcessing() {
  if (workletNode) { workletNode.disconnect(); workletNode = null; }
  if (sourceNode) { sourceNode.disconnect(); sourceNode = null; }
  if (audioCtx) { audioCtx.close(); audioCtx = null; }
  if (mediaStream) { mediaStream.getTracks().forEach(t => t.stop()); mediaStream = null; }
}

function updateConfig(cfg) {
  if (!workletNode) return;
  workletNode.port.postMessage({
    type: 'config',
    strength: (cfg.strength ?? 70) / 100,
    enabled: cfg.enabled !== undefined ? cfg.enabled : true,
    hfReconstruction: (cfg.hfReconstruction ?? 80) / 100,
    dynamics: (cfg.dynamics ?? 60) / 100,
    stylePreset: cfg.stylePreset ?? 0,
  });
}
