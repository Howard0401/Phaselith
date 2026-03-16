<template>
  <div class="app">
    <header>
      <h1>Phaselith</h1>
      <span class="subtitle">CIRRUS Algorithm Audio Enhancement</span>
    </header>

    <section class="status-bar" :class="{ active: isProcessing }">
      <div class="status-indicator">
        <span class="dot" :class="{ active: isProcessing, connected: status && !isProcessing }"></span>
        <span>{{ !status ? 'Disconnected' : isProcessing ? 'Processing' : 'Standby' }}</span>
      </div>
      <div v-if="status" class="status-info">
        <span class="stat-cell">{{ smoothed.cutoff > 100 ? (Math.round(smoothed.cutoff / 500) * 500 / 1000).toFixed(1) + ' kHz' : 'Lossless' }}</span>
        <span class="stat-cell">CPU {{ (smoothed.load / cpuCores).toFixed(2) }}%</span>
        <span class="stat-cell" :class="{ 'algo-active': smoothed.diff > -60 }">
          {{ smoothed.diff > -60 ? 'DSP ' + (smoothed.diff > 0 ? '+' : '') + smoothed.diff.toFixed(1) + ' dB' : 'DSP off' }}
        </span>
      </div>
    </section>

    <section class="controls" :class="{ disabled: !apoInstalled }">
      <div class="toggle-row">
        <label>Enable Enhancement</label>
        <label class="switch" :class="{ disabled: !apoInstalled }">
          <input type="checkbox" v-model="config.enabled" :disabled="!apoInstalled" @change="applyConfig">
          <span class="slider"></span>
        </label>
      </div>

      <SliderControl
        label="Compensation Strength"
        v-model="config.strength"
        :disabled="!apoInstalled"
        @update:modelValue="applyConfig"
      />
      <SliderControl
        label="HF Reconstruction"
        v-model="config.hf_reconstruction"
        :disabled="!apoInstalled"
        @update:modelValue="applyConfig"
      />
      <SliderControl
        label="Dynamics Restoration"
        v-model="config.dynamics"
        :disabled="!apoInstalled"
        @update:modelValue="applyConfig"
      />
      <SliderControl
        label="Transient Repair"
        v-model="config.transient"
        :disabled="!apoInstalled"
        @update:modelValue="applyConfig"
      />

      <!-- Quality/Phase/Synthesis are locked in APO:
           UltraExtreme / Linear / FFT-OLA Pilot -->

      <div class="filter-style-row">
        <label>Filter Style</label>
        <div class="style-pills">
          <button
            v-for="(style, idx) in filterStyles"
            :key="idx"
            class="pill"
            :class="{ active: config.filter_style === idx, disabled: !status }"
            :disabled="!apoInstalled"
            @click="selectPreset(idx)"
          >{{ style }}</button>
          <button
            class="pill"
            :class="{ active: config.filter_style === 3, disabled: !status }"
            :disabled="!apoInstalled"
            @click="config.filter_style = 3; applyConfig()"
          >Custom</button>
        </div>
      </div>

      <!-- Advanced: 6-axis style sliders (debug) -->
      <div class="advanced-toggle" @click="advancedOpen = !advancedOpen">
        <span class="chevron" :class="{ open: advancedOpen }">▸</span>
        <label>Advanced Style Axes</label>
        <span v-if="isCustom" class="custom-badge">Custom</span>
      </div>
      <div v-show="advancedOpen" class="advanced-panel">
        <SliderControl label="Warmth" v-model="config.warmth" :disabled="!apoInstalled" @update:modelValue="onAxisChange" />
        <SliderControl label="Air / Brightness" v-model="config.air_brightness" :disabled="!apoInstalled" @update:modelValue="onAxisChange" />
        <SliderControl label="Smoothness" v-model="config.smoothness" :disabled="!apoInstalled" @update:modelValue="onAxisChange" />
        <SliderControl label="Spatial Spread" v-model="config.spatial_spread" :disabled="!apoInstalled" @update:modelValue="onAxisChange" />
        <SliderControl label="Impact Gain" v-model="config.impact_gain" :disabled="!apoInstalled" @update:modelValue="onAxisChange" />
        <SliderControl label="Body" v-model="config.body" :disabled="!apoInstalled" @update:modelValue="onAxisChange" />
      </div>

    </section>

    <section class="mode-section">
      <div class="mode-header">
        <label>Audio Mode</label>
        <span class="mode-badge" :class="apoInstalled ? 'system' : 'browser'">
          {{ apoInstalled ? 'System-Wide' : 'Chrome Extension' }}
        </span>
      </div>
      <p class="mode-hint" v-if="apoInstalled">
        APO is active. Disable Chrome extension to avoid crackling.
      </p>
      <p class="mode-hint" v-else>
        Use Chrome extension for per-tab enhancement. Install APO for system-wide.
      </p>
      <div class="actions">
        <button v-if="!apoInstalled" class="btn install" @click="installApo">Install APO</button>
        <button v-else class="btn uninstall" @click="uninstallApo">Uninstall APO</button>
      </div>
    </section>

    <footer>Phaselith v0.1.2</footer>
  </div>
</template>

<script setup>
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import SliderControl from './components/SliderControl.vue';

const filterStyles = ['Reference', 'Warm', 'Bass+'];

// Preset values for each filter style (warmth, air_brightness, smoothness, spatial_spread, impact_gain, body)
const presetValues = {
  0: { warmth: 0.15, air_brightness: 0.50, smoothness: 0.40, spatial_spread: 0.30, impact_gain: 0.15, body: 0.40 },
  1: { warmth: 0.55, air_brightness: 0.30, smoothness: 0.60, spatial_spread: 0.30, impact_gain: 0.15, body: 0.50 },
  2: { warmth: 0.20, air_brightness: 0.45, smoothness: 0.35, spatial_spread: 0.30, impact_gain: 0.45, body: 0.75 },
};

const config = ref({
  enabled: true,
  strength: 0.7,
  hf_reconstruction: 0.8,
  dynamics: 0.6,
  transient: 0.5,
  phase_mode: 0,
  quality_preset: 1,
  synthesis_mode: 1,
  filter_style: 0,
  warmth: 0.15,
  air_brightness: 0.50,
  smoothness: 0.40,
  spatial_spread: 0.30,
  impact_gain: 0.15,
  body: 0.40,
});

const cpuCores = navigator.hardwareConcurrency || 1;
const advancedOpen = ref(false);
const isCustom = computed(() => config.value.filter_style === 3);

const status = ref(null);
const apoInstalled = ref(false);
let pollTimer = null;

// Processing = APO connected + enhancement enabled + audio flowing
const isProcessing = computed(() =>
  status.value && config.value.enabled && status.value.processing_load > 0
);

// Smoothed display values — EMA to prevent jumpy numbers
const smoothed = ref({ cutoff: 0, load: 0, diff: -Infinity });
const EMA_ALPHA = 0.08; // lower = smoother, higher = more responsive
function ema(prev, next, alpha = EMA_ALPHA) {
  return prev === 0 || !isFinite(prev) ? next : prev * (1 - alpha) + next * alpha;
}

onMounted(async () => {
  try {
    const saved = await invoke('get_config');
    Object.assign(config.value, saved);
  } catch (e) {
    console.warn('Failed to load config:', e);
  }

  // Only sync config to mmap if IPC is connected.
  // This avoids overwriting user's last toggle state with hardcoded defaults
  // when mmap isn't available (APO not installed → get_config returns defaults).
  try {
    const ipcState = await invoke('get_ipc_state');
    if (ipcState.connected) {
      await applyConfig();
    }
  } catch {
    // IPC not ready — skip initial sync, user will sync manually via UI
  }

  pollTimer = setInterval(pollStatus, 200);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});

async function pollStatus() {
  try {
    const s = await invoke('get_status');
    const wasConnected = !!status.value;
    status.value = s;
    // Smooth the display values so they don't jump every 200ms
    if (s) {
      smoothed.value.cutoff = ema(smoothed.value.cutoff, s.cutoff_freq);
      smoothed.value.load = ema(smoothed.value.load, s.processing_load);
      smoothed.value.diff = s.wet_dry_diff_db > -60
        ? ema(isFinite(smoothed.value.diff) ? smoothed.value.diff : s.wet_dry_diff_db, s.wet_dry_diff_db)
        : -Infinity;
    } else if (wasConnected) {
      // Just disconnected — clear display but keep toggle state
      smoothed.value = { cutoff: 0, load: 0, diff: -Infinity };
    }
  } catch {
    status.value = null;
  }
  try {
    apoInstalled.value = await invoke('is_apo_installed');
  } catch {
    apoInstalled.value = false;
  }
}

function selectPreset(idx) {
  config.value.filter_style = idx;
  // Auto-fill 6 axes from preset values
  if (presetValues[idx]) {
    Object.assign(config.value, presetValues[idx]);
  }
  applyConfig();
}

function onAxisChange() {
  // Any manual slider adjustment switches to Custom
  if (config.value.filter_style !== 3) {
    config.value.filter_style = 3;
  }
  applyConfig();
}

async function applyConfig() {
  try {
    await invoke('set_config', { config: config.value });
  } catch (e) {
    console.error('Failed to set config:', e);
  }
}

async function installApo() {
  const ok = confirm(
    'Install system-wide APO?\n\n' +
    'Important: Disable the Chrome extension before installing.\n' +
    'APO and Chrome extension cannot be active at the same time.'
  );
  if (!ok) return;
  try {
    const msg = await invoke('install_apo');
    apoInstalled.value = true;
    alert(msg);
  } catch (e) {
    alert('Error: ' + e);
  }
}

async function uninstallApo() {
  try {
    const msg = await invoke('uninstall_apo');
    apoInstalled.value = false;
    alert(msg + '\n\nYou can now use the Chrome extension.');
  } catch (e) {
    alert('Error: ' + e);
  }
}
</script>

<style>
* { margin: 0; padding: 0; box-sizing: border-box; }

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  background: #0f0f1a;
  color: #e0e0e0;
}

.app {
  max-width: 420px;
  margin: 0 auto;
  padding: 20px;
}

header {
  text-align: center;
  margin-bottom: 20px;
}
header h1 {
  font-size: 28px;
  color: #00d4ff;
  letter-spacing: 4px;
}
.subtitle {
  font-size: 11px;
  color: #666;
}

.status-bar {
  background: #1a1a2e;
  border-radius: 10px;
  padding: 12px 16px;
  margin-bottom: 20px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  border: 1px solid #2a2a4a;
}
.status-bar.active { border-color: #00d4ff33; }
.status-indicator { display: flex; align-items: center; gap: 8px; }
.dot {
  width: 8px; height: 8px;
  border-radius: 50%;
  background: #555;
}
.dot.active { background: #4ade80; box-shadow: 0 0 8px #4ade8066; }
.dot.connected { background: #facc15; }
.status-info { font-size: 12px; color: #888; display: flex; gap: 12px; }
.stat-cell {
  min-width: 68px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.algo-active { color: #4ade80; font-weight: 600; }

.controls {
  background: #1a1a2e;
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 16px;
}

.toggle-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid #2a2a4a;
}

.switch { position: relative; width: 44px; height: 24px; cursor: pointer; }
.switch input { opacity: 0; width: 0; height: 0; }
.slider {
  position: absolute; inset: 0;
  background: #444; border-radius: 12px;
  transition: 0.3s;
}
.slider::before {
  content: ""; position: absolute;
  height: 18px; width: 18px;
  left: 3px; bottom: 3px;
  background: white; border-radius: 50%;
  transition: 0.3s;
}
input:checked + .slider { background: #00d4ff; }
input:checked + .slider::before { transform: translateX(20px); }
.switch.disabled { opacity: 0.35; cursor: not-allowed; }
.switch.disabled input { pointer-events: none; }
.controls.disabled { opacity: 0.6; }

.filter-style-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid #2a2a4a;
}
.filter-style-row label { font-size: 13px; }
.style-pills { display: flex; gap: 6px; }
.pill {
  padding: 5px 14px;
  border: 1px solid #3a3a5a;
  border-radius: 16px;
  background: transparent;
  color: #aaa;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
  font-weight: 500;
}
.pill:hover:not(.disabled) { border-color: #00d4ff66; color: #e0e0e0; }
.pill.active { background: #00d4ff22; border-color: #00d4ff; color: #00d4ff; font-weight: 600; }
.pill.disabled { opacity: 0.35; cursor: not-allowed; }

.advanced-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px solid #2a2a4a;
  cursor: pointer;
  user-select: none;
}
.advanced-toggle label {
  font-size: 12px;
  color: #888;
  cursor: pointer;
}
.chevron {
  font-size: 11px;
  color: #666;
  transition: transform 0.2s;
  display: inline-block;
}
.chevron.open { transform: rotate(90deg); }
.custom-badge {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 8px;
  background: #00d4ff22;
  color: #00d4ff;
  border: 1px solid #00d4ff44;
  margin-left: auto;
}
.advanced-panel {
  margin-top: 10px;
  padding: 8px 0;
}

.select-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 12px;
}
.select-row label { font-size: 13px; }
.select-row select {
  background: #2a2a4a;
  color: #e0e0e0;
  border: none;
  padding: 4px 8px;
  border-radius: 4px;
  font-size: 12px;
}

.mode-section {
  background: #1a1a2e;
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 16px;
}
.mode-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}
.mode-header label { font-size: 13px; font-weight: 600; }
.mode-badge {
  font-size: 11px;
  padding: 3px 10px;
  border-radius: 12px;
  font-weight: 600;
  letter-spacing: 0.5px;
}
.mode-badge.system { background: #00d4ff22; color: #00d4ff; border: 1px solid #00d4ff44; }
.mode-badge.browser { background: #4ade8022; color: #4ade80; border: 1px solid #4ade8044; }
.mode-hint {
  font-size: 11px;
  color: #888;
  margin-bottom: 12px;
  line-height: 1.5;
}
.actions {
  display: flex;
  gap: 8px;
}
.btn {
  flex: 1;
  padding: 10px;
  border: none;
  border-radius: 8px;
  font-size: 13px;
  cursor: pointer;
  font-weight: 600;
}
.btn.install { background: #00d4ff; color: #000; }
.btn.uninstall { background: #333; color: #e0e0e0; }
.btn:hover { opacity: 0.9; }

footer {
  text-align: center;
  margin-top: 16px;
  font-size: 11px;
  color: #444;
}
</style>
