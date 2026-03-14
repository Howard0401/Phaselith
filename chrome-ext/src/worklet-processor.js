// AudioWorklet processor that runs WASM DSP engine.
//
// This runs on the audio rendering thread (real-time).
// No DOM access, no fetch(), no imports except WASM.
//
// WASM binary is received from offscreen document via port.postMessage
// because AudioWorklet has no fetch() API in Chrome Extensions.
//
// Uses process_block_ch(channel, len) so L/R use separate engine instances
// with independent state (frame counters, smoothers, reprojection history).

class AsceProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.wasmReady = false;
    this.inputPtr = 0;
    this.outputPtr = 0;
    this.heapF32 = null;
    this.wasm = null;

    // Cached config — persisted across startup so nothing is lost
    this.enabled = true;
    this.strength = 0.7;
    this.hfReconstruction = 0.8;
    this.dynamics = 0.6;
    this.stylePreset = 0; // 0=Reference
    this.synthesisMode = 1; // FftOlaPilot (default)

    this.port.onmessage = (e) => {
      const msg = e.data;
      if (msg.type === 'WASM_BINARY') {
        this._initWasm(msg.buffer);
      } else if (msg.type === 'config') {
        // Always cache, even before WASM is ready
        if (msg.enabled !== undefined) this.enabled = msg.enabled;
        if (msg.strength !== undefined) this.strength = msg.strength;
        if (msg.hfReconstruction !== undefined) this.hfReconstruction = msg.hfReconstruction;
        if (msg.dynamics !== undefined) this.dynamics = msg.dynamics;
        if (msg.stylePreset !== undefined) this.stylePreset = msg.stylePreset;
        if (msg.synthesisMode !== undefined) this.synthesisMode = msg.synthesisMode;

        if (this.wasmReady) {
          this._applyConfig();
        }
      }
    };
  }

  _applyConfig() {
    this.wasm.set_strength(this.strength);
    this.wasm.set_enabled(this.enabled ? 1 : 0);
    if (this.wasm.set_hf_reconstruction) {
      this.wasm.set_hf_reconstruction(this.hfReconstruction);
    }
    if (this.wasm.set_dynamics) {
      this.wasm.set_dynamics(this.dynamics);
    }
    if (this.wasm.set_style) {
      this.wasm.set_style(this.stylePreset);
    }
    if (this.wasm.set_synthesis_mode) {
      this.wasm.set_synthesis_mode(this.synthesisMode);
    }
  }

  async _initWasm(wasmBytes) {
    try {
      const { instance } = await WebAssembly.instantiate(wasmBytes, {
        env: { abort: () => {} },
      });

      this.wasm = instance.exports;
      this.wasm.init(sampleRate);

      this.inputPtr = this.wasm.get_input_ptr();
      this.outputPtr = this.wasm.get_output_ptr();
      this.heapF32 = new Float32Array(this.wasm.memory.buffer);

      // Replay all cached config values
      this._applyConfig();

      this.wasmReady = true;
      this.port.postMessage({ type: 'WASM_READY' });
      console.log('CIRRUS WorkletProcessor: WASM loaded');
    } catch (err) {
      console.error('CIRRUS WorkletProcessor: Failed to init WASM:', err);
      this.port.postMessage({ type: 'WASM_ERROR', error: err.message });
    }
  }

  process(inputs, outputs) {
    const input = inputs[0];
    const output = outputs[0];

    if (!input || !input[0]) return true;

    // If WASM not ready or disabled, pass through
    if (!this.wasmReady || !this.enabled) {
      for (let ch = 0; ch < output.length; ch++) {
        if (input[ch]) output[ch].set(input[ch]);
      }
      return true;
    }

    const blockSize = input[0].length;

    for (let ch = 0; ch < Math.min(input.length, output.length); ch++) {
      const inputChannel = input[ch];
      const outputChannel = output[ch];
      if (!inputChannel) continue;

      // Refresh heap view if memory grew
      if (this.heapF32.buffer !== this.wasm.memory.buffer) {
        this.heapF32 = new Float32Array(this.wasm.memory.buffer);
      }

      // Copy input to WASM memory
      const inputOffset = this.inputPtr / 4;
      this.heapF32.set(inputChannel, inputOffset);

      // Process with channel-specific engine (0=L, 1=R)
      this.wasm.process_block_ch(ch, blockSize);

      // Copy output from WASM memory
      const outputOffset = this.outputPtr / 4;
      outputChannel.set(
        this.heapF32.subarray(outputOffset, outputOffset + blockSize)
      );
    }

    return true;
  }
}

registerProcessor('asce-processor', AsceProcessor);
