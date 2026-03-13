// Offscreen document: hosts AudioContext + WASM AudioWorklet
//
// Flow:
//   1. Receives streamId from background
//   2. Creates MediaStream from streamId via getUserMedia constraint hack
//   3. Creates AudioContext → MediaStreamSource → AudioWorkletNode → destination
//   4. AudioWorklet loads WASM and processes audio in real-time

let audioCtx = null;
let sourceNode = null;
let workletNode = null;
let mediaStream = null;

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.type === 'OFFSCREEN_START') {
    startProcessing(msg.streamId);
  } else if (msg.type === 'OFFSCREEN_STOP') {
    stopProcessing();
  } else if (msg.type === 'CONFIG_UPDATE') {
    updateConfig(msg);
  }
});

async function startProcessing(streamId) {
  try {
    // Stop any existing processing
    stopProcessing();

    // Get the tab's audio stream using the streamId
    mediaStream = await navigator.mediaDevices.getUserMedia({
      audio: {
        mandatory: {
          chromeMediaSource: 'tab',
          chromeMediaSourceId: streamId,
        },
      },
    });

    // Create AudioContext at tab's sample rate
    audioCtx = new AudioContext({ sampleRate: 48000 });

    // Load the AudioWorklet processor
    await audioCtx.audioWorklet.addModule('src/worklet-processor.js');

    // Create source from captured stream
    sourceNode = audioCtx.createMediaStreamSource(mediaStream);

    // Create worklet node
    workletNode = new AudioWorkletNode(audioCtx, 'asce-processor', {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [2],
    });

    // Connect: source → ASCE worklet → speakers
    sourceNode.connect(workletNode);
    workletNode.connect(audioCtx.destination);

    // Load saved config
    const data = await chrome.storage.local.get(['strength', 'hfReconstruction', 'dynamics', 'enabled']);
    updateConfig(data);

    console.log('ASCE: Audio processing started');
  } catch (err) {
    console.error('ASCE: Failed to start processing:', err);
  }
}

function stopProcessing() {
  if (workletNode) {
    workletNode.disconnect();
    workletNode = null;
  }
  if (sourceNode) {
    sourceNode.disconnect();
    sourceNode = null;
  }
  if (audioCtx) {
    audioCtx.close();
    audioCtx = null;
  }
  if (mediaStream) {
    mediaStream.getTracks().forEach(t => t.stop());
    mediaStream = null;
  }
  console.log('ASCE: Audio processing stopped');
}

function updateConfig(cfg) {
  if (!workletNode) return;
  workletNode.port.postMessage({
    type: 'config',
    strength: (cfg.strength ?? 70) / 100,
    enabled: cfg.enabled ?? true,
  });
}
