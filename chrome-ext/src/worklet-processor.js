// AudioWorklet processor that runs WASM DSP engine.
//
// This runs on the audio rendering thread (real-time).
// No DOM access, no network, no imports except WASM.
//
// The WASM module exports:
//   init(sampleRate)           - initialize DSP pipeline
//   get_input_ptr() -> ptr     - pointer to input buffer in WASM memory
//   get_output_ptr() -> ptr    - pointer to output buffer in WASM memory
//   process_block(len)         - process audio block
//   set_strength(f32)          - update compensation strength
//   set_enabled(u32)           - enable/disable processing

class AsceProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.wasmReady = false;
    this.enabled = true;
    this.strength = 0.7;
    this.inputPtr = 0;
    this.outputPtr = 0;
    this.heapF32 = null;
    this.wasm = null;

    // Listen for config updates from main thread
    this.port.onmessage = (e) => {
      const msg = e.data;
      if (msg.type === 'config') {
        this.enabled = msg.enabled;
        this.strength = msg.strength;
        if (this.wasmReady) {
          this.wasm.set_strength(this.strength);
          this.wasm.set_enabled(this.enabled ? 1 : 0);
        }
      }
    };

    // Load WASM module
    this._loadWasm();
  }

  async _loadWasm() {
    try {
      // Fetch WASM binary (relative to extension root)
      const response = await fetch(new URL('../asce_wasm_bridge.wasm', import.meta.url));
      const wasmBytes = await response.arrayBuffer();

      // Instantiate with minimal imports (no wasm-bindgen needed)
      const { instance } = await WebAssembly.instantiate(wasmBytes, {
        env: {
          // WASM may need these stubs
          abort: () => {},
        },
      });

      this.wasm = instance.exports;

      // Initialize DSP engine with current sample rate
      this.wasm.init(sampleRate);

      // Get buffer pointers
      this.inputPtr = this.wasm.get_input_ptr();
      this.outputPtr = this.wasm.get_output_ptr();

      // Get WASM linear memory as Float32Array
      this.heapF32 = new Float32Array(this.wasm.memory.buffer);

      // Apply initial config
      this.wasm.set_strength(this.strength);
      this.wasm.set_enabled(this.enabled ? 1 : 0);

      this.wasmReady = true;
      console.log('ASCE WorkletProcessor: WASM loaded');
    } catch (err) {
      console.error('ASCE WorkletProcessor: Failed to load WASM:', err);
    }
  }

  process(inputs, outputs, parameters) {
    const input = inputs[0];
    const output = outputs[0];

    if (!input || !input[0]) return true;

    // If WASM not ready or disabled, pass through
    if (!this.wasmReady || !this.enabled) {
      for (let ch = 0; ch < output.length; ch++) {
        if (input[ch]) {
          output[ch].set(input[ch]);
        }
      }
      return true;
    }

    const blockSize = input[0].length; // Usually 128 samples

    // Process each channel through WASM
    for (let ch = 0; ch < Math.min(input.length, output.length); ch++) {
      const inputChannel = input[ch];
      const outputChannel = output[ch];

      if (!inputChannel) continue;

      // Refresh heap view if memory grew
      if (this.heapF32.buffer !== this.wasm.memory.buffer) {
        this.heapF32 = new Float32Array(this.wasm.memory.buffer);
      }

      // Copy input to WASM memory
      const inputOffset = this.inputPtr / 4; // byte offset to f32 index
      this.heapF32.set(inputChannel, inputOffset);

      // Process
      this.wasm.process_block(blockSize);

      // Copy output from WASM memory
      const outputOffset = this.outputPtr / 4;
      outputChannel.set(
        this.heapF32.subarray(outputOffset, outputOffset + blockSize)
      );
    }

    return true; // Keep processor alive
  }
}

registerProcessor('asce-processor', AsceProcessor);
