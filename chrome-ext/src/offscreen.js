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
let processingStarts = 0;
let currentPlatformOs = 'unknown';
let currentDebugVersion = 'unknown';
const DEFAULT_MAC_TRANSIENT_RUNTIME_MODE = 'transient-safe';
let currentMacTransientRuntimeMode = DEFAULT_MAC_TRANSIENT_RUNTIME_MODE;

function normalizeMacTransientRuntimeMode(mode) {
  switch (mode) {
    case 'hop-probe':
      return 'transient-safe';
    case 'declip-probe':
      return 'declip-safe';
    default:
      return mode;
  }
}

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'OFFSCREEN_START') {
    capturedTabId = msg.tabId ?? null;
    currentPlatformOs = msg.platformOs || 'unknown';
    currentDebugVersion = msg.debugVersion || currentDebugVersion;
    currentMacTransientRuntimeMode = normalizeMacTransientRuntimeMode(
      msg.config?.macTransientMode || currentMacTransientRuntimeMode
    );
    startProcessing(msg.streamId, msg.config);
  } else if (msg.type === 'OFFSCREEN_STOP') {
    capturedTabId = null;
    stopProcessing();
  } else if (msg.type === 'CONFIG_UPDATE') {
    currentPlatformOs = msg.platformOs || currentPlatformOs;
    currentMacTransientRuntimeMode = normalizeMacTransientRuntimeMode(
      msg.macTransientMode || currentMacTransientRuntimeMode
    );
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
    processingStarts += 1;

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

    // Use the device/browser default sample rate to avoid unnecessary
    // resampling artifacts on systems that prefer 44.1kHz, which is common
    // on macOS output devices.
    audioCtx = new AudioContext();
    // Bust Chrome's module cache so extension reloads always pick up the latest
    // AudioWorklet script while we iterate on macOS-specific fixes.
    const workletUrl = `${chrome.runtime.getURL('src/worklet-processor.js')}?v=${Date.now()}`;
    await audioCtx.audioWorklet.addModule(workletUrl);

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
        console.log('CIRRUS: WASM engine ready');
        relayDebugEvent({ type: d.type, debugVersion: currentDebugVersion, platformOs: currentPlatformOs });
        // Worklet caches and replays config on init — no need to resend here.
        // Sending startup config would overwrite newer CONFIG_UPDATE values
        // that arrived while WASM was loading.
      } else if (d.type === 'WORKLET_STATS') {
        console.log('CIRRUS: WORKLET_STATS', d);
        relayDebugEvent(d);
      } else if (d.type === 'WASM_ERROR') {
        console.error('CIRRUS: WASM init error:', d.error);
        relayDebugEvent(d);
      } else if (d.type === 'RUNTIME_ERROR') {
        console.error('CIRRUS: worklet runtime error:', d.error);
        relayDebugEvent(d);
      }
    };

    // Send initial config to worklet so it caches the values before WASM loads.
    // The worklet will replay these cached values in _initWasm() after WASM_READY.
    if (config) {
      updateConfig(config);
    }

    sourceNode.connect(workletNode);
    workletNode.connect(audioCtx.destination);

    if (currentPlatformOs === 'mac') {
      relayDebugEvent({
        type: 'MAC_AUDIO_FIX',
        debugVersion: currentDebugVersion,
        platformOs: currentPlatformOs,
        mode: currentMacTransientRuntimeMode,
      });
    }

    // Send WASM binary to worklet (it can't fetch on its own).
    // Avoid transfer lists here: detached ArrayBuffer behavior has shown up
    // on macOS Chrome, while a structured-cloned Uint8Array stays stable.
    const wasmBytes = new Uint8Array(binary).slice();
    workletNode.port.postMessage({ type: 'WASM_BINARY', bytes: wasmBytes });

    console.log(
      `CIRRUS: Audio processing started (tab ${capturedTabId}, sr=${audioCtx.sampleRate}, starts=${processingStarts}, sourceCh=${sourceNode.channelCount}, outCh=2)`
    );
    relayDebugEvent({
      type: 'OFFSCREEN_START',
      tabId: capturedTabId,
      platformOs: currentPlatformOs,
      debugVersion: currentDebugVersion,
      sampleRate: audioCtx.sampleRate,
      starts: processingStarts,
      sourceChannelCount: sourceNode.channelCount,
      outputChannelCount: 2,
    });
  } catch (err) {
    console.error('CIRRUS: Failed to start processing:', err);
    relayDebugEvent({ type: 'OFFSCREEN_ERROR', error: err?.message || String(err) });
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
  currentMacTransientRuntimeMode = normalizeMacTransientRuntimeMode(
    cfg.macTransientMode || currentMacTransientRuntimeMode
  );
  const isMac = currentPlatformOs === 'mac';
  const useMacStableFallback = isMac && currentMacTransientRuntimeMode === 'stable-fallback';
  const useMacTransientSafe = isMac && currentMacTransientRuntimeMode === 'transient-safe';
  const useMacDeclipOnly =
    isMac
    && (
      currentMacTransientRuntimeMode === 'declip-safe'
      || currentMacTransientRuntimeMode === 'declip-probe'
    );
  const transientValue = (cfg.transient ?? 50) / 100;
  workletNode.port.postMessage({
    type: 'config',
    platformOs: currentPlatformOs,
    debugVersion: currentDebugVersion,
    strength: (cfg.strength ?? 70) / 100,
    enabled: cfg.enabled !== undefined ? cfg.enabled : true,
    hfReconstruction: (cfg.hfReconstruction ?? 80) / 100,
    dynamics: (cfg.dynamics ?? 60) / 100,
    transient: useMacStableFallback ? 0 : transientValue,
    preEchoTransientScaling: useMacStableFallback || useMacDeclipOnly ? 0 : 1,
    declipTransientScaling: useMacTransientSafe ? 0 : 1,
    delayedTransientRepair: useMacTransientSafe,
    macTransientMode: currentMacTransientRuntimeMode,
    stylePreset: cfg.stylePreset ?? 0,
    synthesisMode: cfg.synthesisMode ?? 0,
  });
}

function relayDebugEvent(payload) {
  chrome.runtime.sendMessage({ type: 'DEBUG_EVENT', payload }).catch(() => {});
}
