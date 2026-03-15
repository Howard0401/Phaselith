<template>
  <div class="app">
    <header>
      <h1>Phaselith</h1>
      <span class="subtitle">CIRRUS Algorithm Audio Enhancement</span>
    </header>

    <section class="status-bar" :class="{ active: status }">
      <div class="status-indicator">
        <span class="dot" :class="{ active: status }"></span>
        <span>{{ status ? 'Processing' : 'Idle' }}</span>
      </div>
      <div v-if="status" class="status-info">
        <span>{{ status.cutoff_freq > 0 ? Math.round(status.cutoff_freq) + ' Hz' : 'Lossless' }}</span>
        <span>{{ status.processing_load.toFixed(1) }}%</span>
      </div>
    </section>

    <section class="controls">
      <div class="toggle-row">
        <label>Enable Enhancement</label>
        <label class="switch">
          <input type="checkbox" v-model="config.enabled" @change="applyConfig">
          <span class="slider"></span>
        </label>
      </div>

      <SliderControl
        label="Compensation Strength"
        v-model="config.strength"
        @update:modelValue="applyConfig"
      />
      <SliderControl
        label="HF Reconstruction"
        v-model="config.hf_reconstruction"
        @update:modelValue="applyConfig"
      />
      <SliderControl
        label="Dynamics Restoration"
        v-model="config.dynamics"
        @update:modelValue="applyConfig"
      />
      <SliderControl
        label="Transient Repair"
        v-model="config.transient"
        @update:modelValue="applyConfig"
      />

      <div class="select-row">
        <label>Quality Preset</label>
        <select v-model.number="config.quality_preset" @change="applyConfig">
          <option :value="0">Light (Low CPU)</option>
          <option :value="1">Standard</option>
          <option :value="2">Ultra (High Quality)</option>
        </select>
      </div>

      <div class="select-row">
        <label>Phase Mode</label>
        <select v-model.number="config.phase_mode" @change="applyConfig">
          <option :value="0">Linear Phase</option>
          <option :value="1">Minimum Phase</option>
        </select>
      </div>

      <div class="select-row">
        <label>Synthesis Mode</label>
        <select v-model.number="config.synthesis_mode" @change="applyConfig">
          <option :value="0">Legacy Additive</option>
          <option :value="1">FFT-OLA Pilot</option>
          <option :value="2">FFT-OLA Full</option>
        </select>
      </div>
    </section>

    <section class="actions">
      <button class="btn install" @click="installApo">Install APO</button>
      <button class="btn uninstall" @click="uninstallApo">Uninstall APO</button>
    </section>

    <footer>Phaselith v0.1.0</footer>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import SliderControl from './components/SliderControl.vue';

const config = ref({
  enabled: true,
  strength: 0.7,
  hf_reconstruction: 0.8,
  dynamics: 0.6,
  transient: 0.5,
  phase_mode: 0,
  quality_preset: 1,
  synthesis_mode: 1,
});

const status = ref(null);
let pollTimer = null;

onMounted(async () => {
  try {
    const saved = await invoke('get_config');
    Object.assign(config.value, saved);
  } catch (e) {
    console.warn('Failed to load config:', e);
  }

  pollTimer = setInterval(pollStatus, 200);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});

async function pollStatus() {
  try {
    status.value = await invoke('get_status');
  } catch {
    status.value = null;
  }
}

async function applyConfig() {
  try {
    await invoke('set_config', { config: config.value });
  } catch (e) {
    console.error('Failed to set config:', e);
  }
}

async function installApo() {
  try {
    const msg = await invoke('install_apo');
    alert(msg);
  } catch (e) {
    alert('Error: ' + e);
  }
}

async function uninstallApo() {
  try {
    const msg = await invoke('uninstall_apo');
    alert(msg);
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
.status-info { font-size: 12px; color: #888; display: flex; gap: 12px; }

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
