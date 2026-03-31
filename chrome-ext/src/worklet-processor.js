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
    this.levelInputSqEma = 0;
    this.levelOutputSqEma = 0;
    this.levelMeterFrames = 0;
    this.levelMeterPrimed = false;
    this.analysisWindowFrames = Math.max(1024, Math.round(sampleRate * 0.05));
    this.analysisFrameCount = 0;
    this.analysisSampleCount = 0;
    this.analysisInputEnergy = 0;
    this.analysisOutputEnergy = 0;
    this.analysisDiffEnergy = 0;
    this.analysisInputPeak = 0;
    this.analysisOutputPeak = 0;
    this.analysisHistoryCapacity = 160;
    this.analysisInputRmsDbHistory = [];
    this.analysisOutputRmsDbHistory = [];
    this.analysisInputCrestDbHistory = [];
    this.analysisOutputCrestDbHistory = [];
    this.analysisResidualDbHistory = [];
    this.analysisWaveCorrHistory = [];
    this.analysisLagSamplesHistory = [];
    this.analysisRoughnessDeltaDbHistory = [];
    this.analysisSampleInputWindow = [];
    this.analysisSampleOutputWindow = [];
    this.analysisStereoInputLWindow = [];
    this.analysisStereoInputRWindow = [];
    this.analysisStereoOutputLWindow = [];
    this.analysisStereoOutputRWindow = [];
    this.analysisInputWidthDbHistory = [];
    this.analysisOutputWidthDbHistory = [];
    this.analysisInputBalanceDbHistory = [];
    this.analysisOutputBalanceDbHistory = [];
    this.analysisInputSpacePctHistory = [];
    this.analysisOutputSpacePctHistory = [];
    this.analysisInputEarlyReflPctHistory = [];
    this.analysisOutputEarlyReflPctHistory = [];
    this.earlyReflectionLags = [0.7, 1.5, 3.0, 5.0, 8.0, 12.0]
      .map((ms) => Math.max(1, Math.round(sampleRate * ms / 1000)));

    // Cached config — persisted across startup so nothing is lost
    this.enabled = true;
    this.strength = 1.0;
    this.hfReconstruction = 0.7;
    this.dynamics = 1.0;
    this.transient = 1.0;
    this.preEchoTransientScaling = 1;
    this.declipTransientScaling = 1;
    this.delayedTransientRepair = false;
    this.stylePreset = 0; // 0=Reference
    this.synthesisMode = 1; // FftOlaPilot (default; platform overrides arrive via config)
    this.warmth = 0.5;
    this.airBrightness = 0.5;
    this.smoothness = 0.5;
    this.spatialSpread = 1.0;
    this.impactGain = 1.0;
    this.body = 0.5;
    this.bodyPassEnabled = false;
    this.hfTame = 0.1;
    this.ambiencePreserve = 0.0;
    this.ambienceGlue = 1.0;
    this.bassFlex = 0.3;
    this.maxSubBlock = 1;

    this.port.onmessage = (e) => {
      const msg = e.data;
      if (msg.type === 'WASM_BINARY') {
        this._initWasm(msg.bytes ?? msg.buffer);
      } else if (msg.type === 'REQUEST_ANALYSIS_SNAPSHOT') {
        this.port.postMessage({
          type: 'ANALYSIS_SNAPSHOT',
          payload: this._buildAnalysisSnapshot(),
        });
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
        if (msg.bodyPassEnabled !== undefined) this.bodyPassEnabled = !!msg.bodyPassEnabled;
        if (msg.hfTame !== undefined) this.hfTame = msg.hfTame;
        if (msg.ambiencePreserve !== undefined) this.ambiencePreserve = msg.ambiencePreserve;
        if (msg.ambienceGlue !== undefined) this.ambienceGlue = msg.ambienceGlue;
        if (msg.bassFlex !== undefined) this.bassFlex = msg.bassFlex;
        if (msg.maxSubBlock !== undefined) this.maxSubBlock = msg.maxSubBlock;

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
    if (this.wasm.set_body_pass_enabled) {
      this.wasm.set_body_pass_enabled(this.bodyPassEnabled ? 1 : 0);
    }
    if (this.wasm.set_hf_tame) {
      this.wasm.set_hf_tame(this.hfTame);
    }
    if (this.wasm.set_ambience_preserve && this.ambiencePreserve !== null) {
      this.wasm.set_ambience_preserve(this.ambiencePreserve);
    }
    if (this.wasm.set_ambience_glue) {
      this.wasm.set_ambience_glue(this.ambienceGlue);
    }
    if (this.wasm.set_bass_flex) {
      this.wasm.set_bass_flex(this.bassFlex);
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

  _copySanitizedOutput(outputChannel, inputChannel, frameOffset, outputOffset, chunkSize) {
    let clippedSamples = 0;
    let invalidSamples = 0;
    let peak = 0;
    let sumSq = 0;
    let diffSumSq = 0;

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
      sumSq += sample * sample;

      const inputSample = inputChannel?.[frameOffset + i];
      const cleanInput = Number.isFinite(inputSample) ? inputSample : 0;
      const diff = sample - cleanInput;
      diffSumSq += diff * diff;
    }

    return { peak, clippedSamples, invalidSamples, sumSq, diffSumSq };
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

  _accumulateStats(channelData, frameOffset = 0, frameCount = channelData.length) {
    let sumSq = 0;
    let peak = 0;
    for (let i = frameOffset; i < frameOffset + frameCount; i++) {
      const sample = channelData[i];
      if (!Number.isFinite(sample)) continue;
      sumSq += sample * sample;
      peak = Math.max(peak, Math.abs(sample));
    }
    return { sumSq, peak };
  }

  _updateLiveLevels(inputEnergy, outputEnergy, sampleCount) {
    if (sampleCount <= 0) return;

    const inputSq = inputEnergy / sampleCount;
    const outputSq = outputEnergy / sampleCount;
    const alpha = 0.12;

    if (!this.levelMeterPrimed) {
      this.levelInputSqEma = inputSq;
      this.levelOutputSqEma = outputSq;
      this.levelMeterPrimed = true;
    } else {
      this.levelInputSqEma += alpha * (inputSq - this.levelInputSqEma);
      this.levelOutputSqEma += alpha * (outputSq - this.levelOutputSqEma);
    }

    this.levelMeterFrames += 1;
    if (this.levelMeterFrames < 24) {
      return;
    }
    this.levelMeterFrames = 0;

    const inputDbfs = 10 * Math.log10(Math.max(this.levelInputSqEma, 1e-12));
    const outputDbfs = 10 * Math.log10(Math.max(this.levelOutputSqEma, 1e-12));
    this.port.postMessage({
      type: 'LIVE_LEVELS',
      inputDbfs,
      outputDbfs,
      deltaDb: outputDbfs - inputDbfs,
      enabled: this.enabled,
      processCount: this.processCount,
    });
  }

  _pushAnalysisValue(history, value) {
    if (!Number.isFinite(value)) return;
    if (history.length >= this.analysisHistoryCapacity) {
      history.shift();
    }
    history.push(value);
  }

  _computeMean(values) {
    if (!values.length) return NaN;
    let sum = 0;
    for (let i = 0; i < values.length; i++) {
      sum += values[i];
    }
    return sum / values.length;
  }

  _computePercentile(values, percentile) {
    if (!values.length) return NaN;
    const sorted = [...values].sort((a, b) => a - b);
    const pos = (sorted.length - 1) * percentile;
    const lo = Math.floor(pos);
    const hi = Math.ceil(pos);
    if (lo === hi) return sorted[lo];
    const t = pos - lo;
    return sorted[lo] * (1 - t) + sorted[hi] * t;
  }

  _computeCorrelation(a, b) {
    const n = Math.min(a.length, b.length);
    if (n < 2) return NaN;

    let sumA = 0;
    let sumB = 0;
    for (let i = 0; i < n; i++) {
      sumA += a[i];
      sumB += b[i];
    }
    const meanA = sumA / n;
    const meanB = sumB / n;

    let cov = 0;
    let varA = 0;
    let varB = 0;
    for (let i = 0; i < n; i++) {
      const da = a[i] - meanA;
      const db = b[i] - meanB;
      cov += da * db;
      varA += da * da;
      varB += db * db;
    }

    if (varA <= 1e-9 || varB <= 1e-9) return NaN;
    return cov / Math.sqrt(varA * varB);
  }

  _recordAnalysisSamples(inputChannel, outputChannel, frameOffset, frameCount) {
    if (!inputChannel || !outputChannel || frameCount <= 0) return;

    for (let i = frameOffset; i < frameOffset + frameCount; i++) {
      const inputSample = inputChannel[i];
      const outputSample = outputChannel[i];
      this.analysisSampleInputWindow.push(Number.isFinite(inputSample) ? inputSample : 0);
      this.analysisSampleOutputWindow.push(Number.isFinite(outputSample) ? outputSample : 0);
    }

    const maxSamples = this.analysisWindowFrames * 2;
    if (this.analysisSampleInputWindow.length > maxSamples) {
      this.analysisSampleInputWindow.splice(0, this.analysisSampleInputWindow.length - maxSamples);
    }
    if (this.analysisSampleOutputWindow.length > maxSamples) {
      this.analysisSampleOutputWindow.splice(0, this.analysisSampleOutputWindow.length - maxSamples);
    }
  }

  _recordStereoAnalysisSamples(inputL, inputR, outputL, outputR, frameOffset, frameCount) {
    if (!inputL || !outputL || frameCount <= 0) return;

    const safeInputR = inputR || inputL;
    const safeOutputR = outputR || outputL;

    for (let i = frameOffset; i < frameOffset + frameCount; i++) {
      const inL = inputL[i];
      const inR = safeInputR[i];
      const outL = outputL[i];
      const outR = safeOutputR[i];
      this.analysisStereoInputLWindow.push(Number.isFinite(inL) ? inL : 0);
      this.analysisStereoInputRWindow.push(Number.isFinite(inR) ? inR : 0);
      this.analysisStereoOutputLWindow.push(Number.isFinite(outL) ? outL : 0);
      this.analysisStereoOutputRWindow.push(Number.isFinite(outR) ? outR : 0);
    }

    const maxSamples = this.analysisWindowFrames * 2;
    if (this.analysisStereoInputLWindow.length > maxSamples) {
      this.analysisStereoInputLWindow.splice(0, this.analysisStereoInputLWindow.length - maxSamples);
      this.analysisStereoInputRWindow.splice(0, this.analysisStereoInputRWindow.length - maxSamples);
      this.analysisStereoOutputLWindow.splice(0, this.analysisStereoOutputLWindow.length - maxSamples);
      this.analysisStereoOutputRWindow.splice(0, this.analysisStereoOutputRWindow.length - maxSamples);
    }
  }

  _computeWaveCorrelation(inputSamples, outputSamples) {
    const n = Math.min(inputSamples.length, outputSamples.length);
    if (n < 2) return NaN;

    let dot = 0;
    let inEnergy = 0;
    let outEnergy = 0;
    for (let i = 0; i < n; i++) {
      const input = inputSamples[i];
      const output = outputSamples[i];
      dot += input * output;
      inEnergy += input * input;
      outEnergy += output * output;
    }

    if (inEnergy <= 1e-9 || outEnergy <= 1e-9) return NaN;
    return dot / Math.sqrt(inEnergy * outEnergy);
  }

  _computeBestLagSamples(inputSamples, outputSamples, maxLag = 12) {
    const n = Math.min(inputSamples.length, outputSamples.length);
    if (n <= maxLag * 2 + 4) return NaN;

    let bestLag = 0;
    let bestScore = -Infinity;

    for (let lag = -maxLag; lag <= maxLag; lag++) {
      let dot = 0;
      let inEnergy = 0;
      let outEnergy = 0;
      for (let i = maxLag; i < n - maxLag; i += 2) {
        const outIndex = i + lag;
        const input = inputSamples[i];
        const output = outputSamples[outIndex];
        dot += input * output;
        inEnergy += input * input;
        outEnergy += output * output;
      }
      if (inEnergy <= 1e-9 || outEnergy <= 1e-9) continue;
      const score = dot / Math.sqrt(inEnergy * outEnergy);
      if (score > bestScore) {
        bestScore = score;
        bestLag = lag;
      }
    }

    return bestLag;
  }

  _computeRoughness(inputSamples, outputSamples) {
    const n = Math.min(inputSamples.length, outputSamples.length);
    if (n < 3) {
      return { input: NaN, output: NaN, deltaDb: NaN };
    }

    let inDiffAbs = 0;
    let outDiffAbs = 0;
    let inEnergy = 0;
    let outEnergy = 0;

    for (let i = 1; i < n; i++) {
      const input = inputSamples[i];
      const prevInput = inputSamples[i - 1];
      const output = outputSamples[i];
      const prevOutput = outputSamples[i - 1];
      inDiffAbs += Math.abs(input - prevInput);
      outDiffAbs += Math.abs(output - prevOutput);
      inEnergy += input * input;
      outEnergy += output * output;
    }

    const inputRms = Math.sqrt(inEnergy / Math.max(n - 1, 1));
    const outputRms = Math.sqrt(outEnergy / Math.max(n - 1, 1));
    const inputRoughness = inDiffAbs / Math.max((n - 1) * inputRms, 1e-6);
    const outputRoughness = outDiffAbs / Math.max((n - 1) * outputRms, 1e-6);
    const deltaDb = 20 * Math.log10(
      Math.max(outputRoughness, 1e-6) / Math.max(inputRoughness, 1e-6)
    );

    return {
      input: inputRoughness,
      output: outputRoughness,
      deltaDb,
    };
  }

  _computeStereoProxy(leftSamples, rightSamples) {
    const n = Math.min(leftSamples.length, rightSamples.length);
    if (n < 2) {
      return {
        widthDb: NaN,
        balanceDb: NaN,
        spacePct: NaN,
        earlyReflectionPct: NaN,
      };
    }

    let leftEnergy = 0;
    let rightEnergy = 0;
    let midEnergy = 0;
    let sideEnergy = 0;
    const mid = new Array(n);

    for (let i = 0; i < n; i++) {
      const left = leftSamples[i];
      const right = rightSamples[i];
      const midSample = 0.5 * (left + right);
      const sideSample = 0.5 * (left - right);
      mid[i] = midSample;
      leftEnergy += left * left;
      rightEnergy += right * right;
      midEnergy += midSample * midSample;
      sideEnergy += sideSample * sideSample;
    }

    const widthDb = 10 * Math.log10(
      Math.max(sideEnergy, 1e-12) / Math.max(midEnergy, 1e-12)
    );
    const leftRms = Math.sqrt(leftEnergy / n);
    const rightRms = Math.sqrt(rightEnergy / n);
    const balanceDb = 20 * Math.log10(
      Math.max(leftRms, 1e-6) / Math.max(rightRms, 1e-6)
    );
    const lrCorr = this._computeWaveCorrelation(leftSamples, rightSamples);
    const sideShare = sideEnergy / Math.max(midEnergy + sideEnergy, 1e-12);
    const spacePct = 100 * sideShare * (1 - Math.min(Math.abs(lrCorr), 1));

    let reflAccum = 0;
    let reflCount = 0;
    for (let i = 0; i < this.earlyReflectionLags.length; i++) {
      const lag = this.earlyReflectionLags[i];
      if (lag >= n - 4) continue;
      let dot = 0;
      let aEnergy = 0;
      let bEnergy = 0;
      for (let j = lag; j < n; j += 2) {
        const a = mid[j];
        const b = mid[j - lag];
        dot += a * b;
        aEnergy += a * a;
        bEnergy += b * b;
      }
      if (aEnergy <= 1e-9 || bEnergy <= 1e-9) continue;
      reflAccum += Math.abs(dot / Math.sqrt(aEnergy * bEnergy));
      reflCount += 1;
    }

    return {
      widthDb,
      balanceDb,
      spacePct,
      earlyReflectionPct: reflCount > 0 ? (reflAccum / reflCount) * 100 : NaN,
    };
  }

  _flushSampleAnalysisWindow() {
    if (this.analysisSampleInputWindow.length < this.analysisWindowFrames) {
      return;
    }

    const inputSamples = this.analysisSampleInputWindow.slice(-this.analysisWindowFrames);
    const outputSamples = this.analysisSampleOutputWindow.slice(-this.analysisWindowFrames);
    const waveCorr = this._computeWaveCorrelation(inputSamples, outputSamples);
    const lagSamples = this._computeBestLagSamples(inputSamples, outputSamples);
    const roughness = this._computeRoughness(inputSamples, outputSamples);

    this._pushAnalysisValue(this.analysisWaveCorrHistory, waveCorr);
    this._pushAnalysisValue(this.analysisLagSamplesHistory, lagSamples);
    this._pushAnalysisValue(this.analysisRoughnessDeltaDbHistory, roughness.deltaDb);

    this.analysisSampleInputWindow.length = 0;
    this.analysisSampleOutputWindow.length = 0;
  }

  _flushStereoAnalysisWindow() {
    if (this.analysisStereoInputLWindow.length < this.analysisWindowFrames) {
      return;
    }

    const inputL = this.analysisStereoInputLWindow.slice(-this.analysisWindowFrames);
    const inputR = this.analysisStereoInputRWindow.slice(-this.analysisWindowFrames);
    const outputL = this.analysisStereoOutputLWindow.slice(-this.analysisWindowFrames);
    const outputR = this.analysisStereoOutputRWindow.slice(-this.analysisWindowFrames);

    const inputProxy = this._computeStereoProxy(inputL, inputR);
    const outputProxy = this._computeStereoProxy(outputL, outputR);

    this._pushAnalysisValue(this.analysisInputWidthDbHistory, inputProxy.widthDb);
    this._pushAnalysisValue(this.analysisOutputWidthDbHistory, outputProxy.widthDb);
    this._pushAnalysisValue(this.analysisInputBalanceDbHistory, inputProxy.balanceDb);
    this._pushAnalysisValue(this.analysisOutputBalanceDbHistory, outputProxy.balanceDb);
    this._pushAnalysisValue(this.analysisInputSpacePctHistory, inputProxy.spacePct);
    this._pushAnalysisValue(this.analysisOutputSpacePctHistory, outputProxy.spacePct);
    this._pushAnalysisValue(this.analysisInputEarlyReflPctHistory, inputProxy.earlyReflectionPct);
    this._pushAnalysisValue(this.analysisOutputEarlyReflPctHistory, outputProxy.earlyReflectionPct);

    this.analysisStereoInputLWindow.length = 0;
    this.analysisStereoInputRWindow.length = 0;
    this.analysisStereoOutputLWindow.length = 0;
    this.analysisStereoOutputRWindow.length = 0;
  }

  _recordAnalysisWindow(inputEnergy, outputEnergy, diffEnergy, sampleCount, inputPeak, outputPeak, frameCount) {
    if (frameCount <= 0 || sampleCount <= 0) return;

    this.analysisFrameCount += frameCount;
    this.analysisSampleCount += sampleCount;
    this.analysisInputEnergy += inputEnergy;
    this.analysisOutputEnergy += outputEnergy;
    this.analysisDiffEnergy += diffEnergy;
    this.analysisInputPeak = Math.max(this.analysisInputPeak, inputPeak);
    this.analysisOutputPeak = Math.max(this.analysisOutputPeak, outputPeak);

    if (this.analysisFrameCount < this.analysisWindowFrames) {
      return;
    }

    const inputRms = Math.sqrt(this.analysisInputEnergy / Math.max(this.analysisSampleCount, 1));
    const outputRms = Math.sqrt(this.analysisOutputEnergy / Math.max(this.analysisSampleCount, 1));
    const inputRmsDb = 20 * Math.log10(Math.max(inputRms, 1e-6));
    const outputRmsDb = 20 * Math.log10(Math.max(outputRms, 1e-6));
    const inputCrestDb = 20 * Math.log10(
      Math.max(this.analysisInputPeak / Math.max(inputRms, 1e-6), 1e-6)
    );
    const outputCrestDb = 20 * Math.log10(
      Math.max(this.analysisOutputPeak / Math.max(outputRms, 1e-6), 1e-6)
    );
    const residualDb = 10 * Math.log10(
      Math.max(this.analysisDiffEnergy, 1e-12) / Math.max(this.analysisInputEnergy, 1e-12)
    );

    this._pushAnalysisValue(this.analysisInputRmsDbHistory, inputRmsDb);
    this._pushAnalysisValue(this.analysisOutputRmsDbHistory, outputRmsDb);
    this._pushAnalysisValue(this.analysisInputCrestDbHistory, inputCrestDb);
    this._pushAnalysisValue(this.analysisOutputCrestDbHistory, outputCrestDb);
    this._pushAnalysisValue(this.analysisResidualDbHistory, residualDb);
    this._flushSampleAnalysisWindow();
    this._flushStereoAnalysisWindow();

    this.analysisFrameCount = 0;
    this.analysisSampleCount = 0;
    this.analysisInputEnergy = 0;
    this.analysisOutputEnergy = 0;
    this.analysisDiffEnergy = 0;
    this.analysisInputPeak = 0;
    this.analysisOutputPeak = 0;
  }

  _buildAnalysisSnapshot() {
    const historyCount = Math.min(
      this.analysisInputRmsDbHistory.length,
      this.analysisOutputRmsDbHistory.length,
      this.analysisInputCrestDbHistory.length,
      this.analysisOutputCrestDbHistory.length,
      this.analysisResidualDbHistory.length,
      this.analysisWaveCorrHistory.length,
      this.analysisLagSamplesHistory.length,
      this.analysisRoughnessDeltaDbHistory.length,
      this.analysisInputWidthDbHistory.length,
      this.analysisOutputWidthDbHistory.length,
      this.analysisInputBalanceDbHistory.length,
      this.analysisOutputBalanceDbHistory.length,
      this.analysisInputSpacePctHistory.length,
      this.analysisOutputSpacePctHistory.length,
      this.analysisInputEarlyReflPctHistory.length,
      this.analysisOutputEarlyReflPctHistory.length
    );

    const inputRmsDb = this.analysisInputRmsDbHistory.slice(-historyCount);
    const outputRmsDb = this.analysisOutputRmsDbHistory.slice(-historyCount);
    const inputCrestDb = this.analysisInputCrestDbHistory.slice(-historyCount);
    const outputCrestDb = this.analysisOutputCrestDbHistory.slice(-historyCount);
    const residualDb = this.analysisResidualDbHistory.slice(-historyCount);
    const waveCorr = this.analysisWaveCorrHistory.slice(-historyCount);
    const lagSamples = this.analysisLagSamplesHistory.slice(-historyCount);
    const roughnessDeltaDb = this.analysisRoughnessDeltaDbHistory.slice(-historyCount);
    const inputWidthDb = this.analysisInputWidthDbHistory.slice(-historyCount);
    const outputWidthDb = this.analysisOutputWidthDbHistory.slice(-historyCount);
    const inputBalanceDb = this.analysisInputBalanceDbHistory.slice(-historyCount);
    const outputBalanceDb = this.analysisOutputBalanceDbHistory.slice(-historyCount);
    const inputSpacePct = this.analysisInputSpacePctHistory.slice(-historyCount);
    const outputSpacePct = this.analysisOutputSpacePctHistory.slice(-historyCount);
    const inputEarlyReflPct = this.analysisInputEarlyReflPctHistory.slice(-historyCount);
    const outputEarlyReflPct = this.analysisOutputEarlyReflPctHistory.slice(-historyCount);

    let inputDynRangeDb = NaN;
    let outputDynRangeDb = NaN;
    let dynRangeDeltaDb = NaN;
    let inputCrestMeanDb = NaN;
    let outputCrestMeanDb = NaN;
    let crestDeltaDb = NaN;
    let envelopeCorrelation = NaN;
    let residualMeanDb = NaN;
    let waveCorrMean = NaN;
    let lagMeanSamples = NaN;
    let lagAbsMeanSamples = NaN;
    let roughnessDeltaMeanDb = NaN;
    let widthDeltaMeanDb = NaN;
    let balanceDeltaMeanDb = NaN;
    let balanceAbsDeltaMeanDb = NaN;
    let inputSpaceMeanPct = NaN;
    let outputSpaceMeanPct = NaN;
    let spaceDeltaMeanPct = NaN;
    let inputEarlyReflMeanPct = NaN;
    let outputEarlyReflMeanPct = NaN;
    let earlyReflDeltaMeanPct = NaN;

    if (historyCount >= 8) {
      inputDynRangeDb =
        this._computePercentile(inputRmsDb, 0.9) - this._computePercentile(inputRmsDb, 0.1);
      outputDynRangeDb =
        this._computePercentile(outputRmsDb, 0.9) - this._computePercentile(outputRmsDb, 0.1);
      dynRangeDeltaDb = outputDynRangeDb - inputDynRangeDb;
      inputCrestMeanDb = this._computeMean(inputCrestDb);
      outputCrestMeanDb = this._computeMean(outputCrestDb);
      crestDeltaDb = outputCrestMeanDb - inputCrestMeanDb;
      envelopeCorrelation = this._computeCorrelation(inputRmsDb, outputRmsDb);
      residualMeanDb = this._computeMean(residualDb);
      waveCorrMean = this._computeMean(waveCorr);
      lagMeanSamples = this._computeMean(lagSamples);
      lagAbsMeanSamples = this._computeMean(lagSamples.map((value) => Math.abs(value)));
      roughnessDeltaMeanDb = this._computeMean(roughnessDeltaDb);
      widthDeltaMeanDb = this._computeMean(
        outputWidthDb.map((value, index) => value - inputWidthDb[index])
      );
      balanceDeltaMeanDb = this._computeMean(
        outputBalanceDb.map((value, index) => value - inputBalanceDb[index])
      );
      balanceAbsDeltaMeanDb = this._computeMean(
        outputBalanceDb.map((value, index) => Math.abs(value - inputBalanceDb[index]))
      );
      inputSpaceMeanPct = this._computeMean(inputSpacePct);
      outputSpaceMeanPct = this._computeMean(outputSpacePct);
      spaceDeltaMeanPct = outputSpaceMeanPct - inputSpaceMeanPct;
      inputEarlyReflMeanPct = this._computeMean(inputEarlyReflPct);
      outputEarlyReflMeanPct = this._computeMean(outputEarlyReflPct);
      earlyReflDeltaMeanPct = outputEarlyReflMeanPct - inputEarlyReflMeanPct;
    }

    return {
      enoughData: historyCount >= 8,
      historyCount,
      windowMs: (this.analysisWindowFrames / sampleRate) * 1000,
      historySeconds: (historyCount * this.analysisWindowFrames) / sampleRate,
      inputRmsDb,
      outputRmsDb,
      inputCrestDb,
      outputCrestDb,
      residualDb,
      waveCorr,
      lagSamples,
      roughnessDeltaDb,
      inputWidthDb,
      outputWidthDb,
      inputBalanceDb,
      outputBalanceDb,
      inputSpacePct,
      outputSpacePct,
      inputEarlyReflPct,
      outputEarlyReflPct,
      inputDynRangeDb,
      outputDynRangeDb,
      dynRangeDeltaDb,
      inputCrestMeanDb,
      outputCrestMeanDb,
      crestDeltaDb,
      envelopeCorrelation,
      residualMeanDb,
      waveCorrMean,
      lagMeanSamples,
      lagAbsMeanSamples,
      roughnessDeltaMeanDb,
      widthDeltaMeanDb,
      balanceDeltaMeanDb,
      balanceAbsDeltaMeanDb,
      inputSpaceMeanPct,
      outputSpaceMeanPct,
      spaceDeltaMeanPct,
      inputEarlyReflMeanPct,
      outputEarlyReflMeanPct,
      earlyReflDeltaMeanPct,
      sampleRate,
      processCount: this.processCount,
    };
  }

  async _initWasm(wasmPayload) {
    try {
      const wasmBytes = this._normalizeWasmBytes(wasmPayload);
      const { instance } = await WebAssembly.instantiate(wasmBytes, {
        env: { abort: () => {} },
      });

      this.wasm = instance.exports;
      const maxSubBlock = this.platformOs === 'mac'
        ? (this.maxSubBlock === 8 ? 8 : 1)
        : 1;
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
      let inputEnergy = 0;
      let outputEnergy = 0;
      let diffEnergy = 0;
      let energySamples = 0;
      let inputPeak = 0;
      let outputPeak = 0;
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
          const inputStats = this._accumulateStats(inputChannel, frameOffset, chunkSize);
          inputEnergy += inputStats.sumSq;
          inputPeak = Math.max(inputPeak, inputStats.peak);

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
            inputChannel,
            frameOffset,
            outputOffset,
            chunkSize
          );
          outputEnergy += safety.sumSq;
          diffEnergy += safety.diffSumSq;
          outputPeak = Math.max(outputPeak, safety.peak);
          energySamples += chunkSize;

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

          if (ch === 0) {
            this._recordAnalysisSamples(inputChannel, outputChannel, frameOffset, chunkSize);
          }
        }

        this._recordStereoAnalysisSamples(
          input[0],
          input[1],
          output[0],
          output[1],
          frameOffset,
          chunkSize
        );
      }

      this._updateLiveLevels(inputEnergy, outputEnergy, energySamples);
      this._recordAnalysisWindow(
        inputEnergy,
        outputEnergy,
        diffEnergy,
        energySamples,
        inputPeak,
        outputPeak,
        blockSize
      );

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
