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

class PhaselithProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.wasmReady = false;
    this.inputPtr = 0;
    this.outputPtr = 0;
    this.heapF32 = null;
    this.wasm = null;
    this.runtimeErrorLogged = false;
    this.blockSizeWarningLogged = false;
    this.maxWasmBlockFrames = 128;
    this.processCount = 0;
    this.statsPosted = false;
    this.safetyEventCount = 0;
    this.platformOs = 'unknown';
    this.timingEventCount = 0;
    this.macProcessLogCount = 0;
    this.debugVersion = 'unknown';

    // Cached config — persisted across startup so nothing is lost
    this.enabled = true;
    this.strength = 0.7;
    this.hfReconstruction = 0.8;
    this.dynamics = 0.6;
    this.transient = null;
    this.preEchoTransientScaling = 1;
    this.declipTransientScaling = 1;
    this.delayedTransientRepair = false;
    this.stylePreset = 0; // 0=Reference
    this.synthesisMode = 1; // FftOlaPilot (default; platform overrides arrive via config)
    this.warmth = null;
    this.airBrightness = null;
    this.smoothness = null;
    this.spatialSpread = null;
    this.impactGain = null;
    this.body = null;
    this.ambiencePreserve = null;

    this.port.onmessage = (e) => {
      const msg = e.data;
      if (msg.type === 'WASM_BINARY') {
        this._initWasm(msg.bytes ?? msg.buffer);
      } else if (msg.type === 'config') {
        // Always cache, even before WASM is ready
        if (msg.platformOs) this.platformOs = msg.platformOs;
        if (msg.debugVersion) this.debugVersion = msg.debugVersion;
        if (msg.enabled !== undefined) this.enabled = msg.enabled;
        if (msg.strength !== undefined) this.strength = msg.strength;
        if (msg.hfReconstruction !== undefined) this.hfReconstruction = msg.hfReconstruction;
        if (msg.dynamics !== undefined) this.dynamics = msg.dynamics;
        if (msg.transient !== undefined) this.transient = msg.transient;
        if (msg.preEchoTransientScaling !== undefined) {
          this.preEchoTransientScaling = msg.preEchoTransientScaling;
        }
        if (msg.declipTransientScaling !== undefined) {
          this.declipTransientScaling = msg.declipTransientScaling;
        }
        if (msg.delayedTransientRepair !== undefined) {
          this.delayedTransientRepair = !!msg.delayedTransientRepair;
        }
        if (msg.stylePreset !== undefined) this.stylePreset = msg.stylePreset;
        if (msg.synthesisMode !== undefined) this.synthesisMode = msg.synthesisMode;
        if (msg.warmth !== undefined) this.warmth = msg.warmth;
        if (msg.airBrightness !== undefined) this.airBrightness = msg.airBrightness;
        if (msg.smoothness !== undefined) this.smoothness = msg.smoothness;
        if (msg.spatialSpread !== undefined) this.spatialSpread = msg.spatialSpread;
        if (msg.impactGain !== undefined) this.impactGain = msg.impactGain;
        if (msg.body !== undefined) this.body = msg.body;
        if (msg.ambiencePreserve !== undefined) this.ambiencePreserve = msg.ambiencePreserve;

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
    if (this.wasm.set_transient && this.transient !== null) {
      this.wasm.set_transient(this.transient);
    }
    if (this.wasm.set_pre_echo_transient_scaling) {
      this.wasm.set_pre_echo_transient_scaling(this.preEchoTransientScaling);
    }
    if (this.wasm.set_declip_transient_scaling) {
      this.wasm.set_declip_transient_scaling(this.declipTransientScaling);
    }
    if (this.wasm.set_delayed_transient_repair) {
      this.wasm.set_delayed_transient_repair(this.delayedTransientRepair ? 1 : 0);
    }
    if (this.wasm.set_style) {
      this.wasm.set_style(this.stylePreset);
    }
    if (this.wasm.set_warmth && this.warmth !== null) {
      this.wasm.set_warmth(this.warmth);
    }
    if (this.wasm.set_air_brightness && this.airBrightness !== null) {
      this.wasm.set_air_brightness(this.airBrightness);
    }
    if (this.wasm.set_smoothness && this.smoothness !== null) {
      this.wasm.set_smoothness(this.smoothness);
    }
    if (this.wasm.set_spatial_spread && this.spatialSpread !== null) {
      this.wasm.set_spatial_spread(this.spatialSpread);
    }
    if (this.wasm.set_impact_gain && this.impactGain !== null) {
      this.wasm.set_impact_gain(this.impactGain);
    }
    if (this.wasm.set_body && this.body !== null) {
      this.wasm.set_body(this.body);
    }
    if (this.wasm.set_ambience_preserve && this.ambiencePreserve !== null) {
      this.wasm.set_ambience_preserve(this.ambiencePreserve);
    }
    if (this.wasm.set_synthesis_mode) {
      this.wasm.set_synthesis_mode(this.synthesisMode);
    }
  }

  _normalizeWasmBytes(payload) {
    if (payload instanceof Uint8Array) {
      return payload.slice();
    }
    if (payload instanceof ArrayBuffer) {
      return new Uint8Array(payload.slice(0));
    }
    if (ArrayBuffer.isView(payload)) {
      const start = payload.byteOffset;
      const end = start + payload.byteLength;
      return new Uint8Array(payload.buffer.slice(start, end));
    }
    throw new Error('Unsupported WASM payload type');
  }

  _syncHeapView(force = false) {
    const memoryBuffer = this.wasm?.memory?.buffer;
    if (!memoryBuffer) {
      throw new Error('WASM memory unavailable');
    }
    if (force || !this.heapF32 || this.heapF32.buffer !== memoryBuffer) {
      this.heapF32 = new Float32Array(memoryBuffer);
    }
  }

  _copySanitizedOutput(outputChannel, frameOffset, outputOffset, chunkSize) {
    let clippedSamples = 0;
    let invalidSamples = 0;
    let peak = 0;

    for (let i = 0; i < chunkSize; i++) {
      const raw = this.heapF32[outputOffset + i];
      let sample = raw;

      if (!Number.isFinite(sample)) {
        sample = 0;
        invalidSamples += 1;
      } else if (sample > 1.0) {
        sample = 1.0;
        clippedSamples += 1;
      } else if (sample < -1.0) {
        sample = -1.0;
        clippedSamples += 1;
      }

      peak = Math.max(peak, Math.abs(sample));
      outputChannel[frameOffset + i] = sample;
    }

    return { peak, clippedSamples, invalidSamples };
  }

  _computePeak(channelData) {
    let peak = 0;
    for (let i = 0; i < channelData.length; i++) {
      const sample = channelData[i];
      if (!Number.isFinite(sample)) {
        return { peak: 0, invalid: true };
      }
      peak = Math.max(peak, Math.abs(sample));
    }
    return { peak, invalid: false };
  }

  async _initWasm(wasmPayload) {
    try {
      const wasmBytes = this._normalizeWasmBytes(wasmPayload);
      const { instance } = await WebAssembly.instantiate(wasmBytes, {
        env: { abort: () => {} },
      });

      this.wasm = instance.exports;
      const maxSubBlock = this.platformOs === 'mac' ? 4 : 1;
      if (this.wasm.init_with_sub_block) {
        this.wasm.init_with_sub_block(sampleRate, maxSubBlock);
      } else {
        this.wasm.init(sampleRate);
      }

      this.inputPtr = this.wasm.get_input_ptr();
      this.outputPtr = this.wasm.get_output_ptr();
      this._syncHeapView(true);

      this.port.postMessage({
        type: 'WORKLET_STATS',
        stage: 'init',
        platformOs: this.platformOs,
        debugVersion: this.debugVersion,
        sampleRate,
        maxSubBlock,
        inputPtr: this.inputPtr,
        outputPtr: this.outputPtr,
        memoryBytes: this.wasm.memory?.buffer?.byteLength ?? 0,
      });

      // Replay all cached config values
      this._applyConfig();

      this.wasmReady = true;
      this.port.postMessage({ type: 'WASM_READY' });
      console.log('Phaselith WorkletProcessor: WASM loaded');
    } catch (err) {
      console.error('Phaselith WorkletProcessor: Failed to init WASM:', err);
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

    try {
      const blockSize = input[0].length;
      const processStartMs = globalThis.performance?.now?.() ?? 0;
      this.processCount += 1;
      if (blockSize > this.maxWasmBlockFrames && !this.blockSizeWarningLogged) {
        this.blockSizeWarningLogged = true;
        console.warn(
          `Phaselith WorkletProcessor: render quantum ${blockSize} exceeds WASM bridge block size ${this.maxWasmBlockFrames}, chunking audio`
        );
      }

      for (let frameOffset = 0; frameOffset < blockSize; frameOffset += this.maxWasmBlockFrames) {
        const chunkSize = Math.min(this.maxWasmBlockFrames, blockSize - frameOffset);

        for (let ch = 0; ch < Math.min(input.length, output.length); ch++) {
          const inputChannel = input[ch];
          const outputChannel = output[ch];
          if (!inputChannel) continue;

          const inputOffset = this.inputPtr / 4;
          const outputOffset = this.outputPtr / 4;

          this._syncHeapView();

          // Keep chunk ordering L->R for each slice so the bridge's
          // cross-channel state advances in the same pattern as the
          // non-chunked render path.
          this.heapF32.set(
            inputChannel.subarray(frameOffset, frameOffset + chunkSize),
            inputOffset
          );

          // Processing can grow wasm memory; refresh the heap view afterwards too.
          this.wasm.process_block_ch(ch, chunkSize);
          this._syncHeapView();

          const safety = this._copySanitizedOutput(
            outputChannel,
            frameOffset,
            outputOffset,
            chunkSize
          );

          if ((safety.clippedSamples > 0 || safety.invalidSamples > 0) && this.safetyEventCount < 8) {
            this.safetyEventCount += 1;
            this.port.postMessage({
              type: 'WORKLET_STATS',
              stage: 'safety',
              platformOs: this.platformOs,
              debugVersion: this.debugVersion,
              processCount: this.processCount,
              channel: ch,
              frameOffset,
              chunkSize,
              peak: safety.peak,
              clippedSamples: safety.clippedSamples,
              invalidSamples: safety.invalidSamples,
            });
          }
        }
      }

      if (!this.statsPosted && this.processCount <= 4) {
        for (let ch = 0; ch < Math.min(input.length, output.length); ch++) {
          const outputChannel = output[ch];
          if (!outputChannel) continue;

          const { peak, invalid } = this._computePeak(outputChannel);
          this.port.postMessage({
            type: 'WORKLET_STATS',
            stage: 'process',
            platformOs: this.platformOs,
            debugVersion: this.debugVersion,
            processCount: this.processCount,
            channel: ch,
            blockSize,
            peak,
            invalid,
            memoryBytes: this.wasm.memory?.buffer?.byteLength ?? 0,
          });
        }
      }

      const shouldEmitMacTrace = this.platformOs === 'mac'
        && this.macProcessLogCount < 24
        && (
          this.processCount <= 12
          || this.processCount % 32 === 0
        );

      if (shouldEmitMacTrace) {
        this.macProcessLogCount += 1;
        for (let ch = 0; ch < Math.min(input.length, output.length); ch++) {
          const inputChannel = input[ch];
          const outputChannel = output[ch];
          if (!inputChannel || !outputChannel) continue;

          const inputStats = this._computePeak(inputChannel);
          const outputStats = this._computePeak(outputChannel);
          this.port.postMessage({
            type: 'WORKLET_STATS',
            stage: 'mac-trace',
            platformOs: this.platformOs,
            debugVersion: this.debugVersion,
            processCount: this.processCount,
            channel: ch,
            blockSize,
            inputPeak: inputStats.peak,
            inputInvalid: inputStats.invalid,
            outputPeak: outputStats.peak,
            outputInvalid: outputStats.invalid,
          });
        }
      }

      const processElapsedMs = (globalThis.performance?.now?.() ?? processStartMs) - processStartMs;
      const processBudgetMs = (blockSize / sampleRate) * 1000;
      if (this.timingEventCount < 8) {
        this.timingEventCount += 1;
        this.port.postMessage({
          type: 'WORKLET_STATS',
          stage: 'timing',
          platformOs: this.platformOs,
          debugVersion: this.debugVersion,
          processCount: this.processCount,
          blockSize,
          elapsedMs: processElapsedMs,
          budgetMs: processBudgetMs,
          overBudget: processElapsedMs > processBudgetMs,
        });
      }

      if (!this.statsPosted && this.processCount >= 4) {
        this.statsPosted = true;
      }

      return true;
    } catch (err) {
      if (!this.runtimeErrorLogged) {
        this.runtimeErrorLogged = true;
        console.error('Phaselith WorkletProcessor: runtime fallback to pass-through:', err);
        this.port.postMessage({
          type: 'RUNTIME_ERROR',
          error: err?.message || String(err),
        });
      }
      for (let ch = 0; ch < output.length; ch++) {
        if (input[ch]) output[ch].set(input[ch]);
      }
      return true;
    }
  }
}

registerProcessor('phaselith-processor', PhaselithProcessor);
