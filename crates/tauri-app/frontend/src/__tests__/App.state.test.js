/**
 * UI State Gate Tests — Phaselith Control Panel
 *
 * Verifies all three UI states and transitions:
 *   Disconnected → Standby → Processing → Disconnected
 *
 * These are component-level integration tests that mock the Tauri IPC layer
 * and assert DOM state (disabled, classes, text) for each scenario.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import App from '../App.vue';

// ---------------------------------------------------------------------------
// Mock @tauri-apps/api/core — intercept invoke() calls
// ---------------------------------------------------------------------------
const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args) => mockInvoke(...args),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Simulate a status snapshot from the APO mmap bridge */
function makeStatus(overrides = {}) {
  return {
    frame_count: 100,
    cutoff_freq: 15000,
    quality_tier: 4,
    clipping: 0,
    processing_load: 5.2,
    wet_dry_diff_db: -3.5,
    ...overrides,
  };
}

/** Default invoke handler — disconnected state (no APO) */
function disconnectedHandler(cmd) {
  if (cmd === 'get_config') return Promise.resolve({
    enabled: true, strength: 0.7, hf_reconstruction: 0.8,
    dynamics: 0.6, transient: 0.5, phase_mode: 0,
    quality_preset: 1, synthesis_mode: 1,
  });
  if (cmd === 'get_ipc_state') return Promise.resolve({ connected: false });
  if (cmd === 'get_status') return Promise.resolve(null);
  if (cmd === 'is_apo_installed') return Promise.resolve(false);
  if (cmd === 'set_config') return Promise.resolve();
  return Promise.reject(`Unknown command: ${cmd}`);
}

/** Invoke handler — standby state (APO installed, enabled, no audio) */
function standbyHandler(cmd) {
  if (cmd === 'get_status') return Promise.resolve(makeStatus({ processing_load: 0 }));
  if (cmd === 'is_apo_installed') return Promise.resolve(true);
  return disconnectedHandler(cmd);
}

/** Invoke handler — processing state (APO installed, enabled, audio flowing) */
function processingHandler(cmd) {
  if (cmd === 'get_status') return Promise.resolve(makeStatus({ processing_load: 5.2 }));
  if (cmd === 'is_apo_installed') return Promise.resolve(true);
  return disconnectedHandler(cmd);
}

async function mountApp() {
  const wrapper = mount(App, {
    global: {
      stubs: { SliderControl: true },
    },
  });
  await flushPromises();
  return wrapper;
}

/** Trigger one poll cycle (simulates the 200ms setInterval) */
async function triggerPoll(wrapper) {
  // Call pollStatus by triggering the interval callback
  // Since we can't directly access setInterval, we advance timers
  await vi.advanceTimersByTimeAsync(200);
  await flushPromises();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('UI State Machine', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockInvoke.mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // =========================================================================
  // STATE 1: Disconnected
  // =========================================================================
  describe('Disconnected state', () => {
    beforeEach(() => {
      mockInvoke.mockImplementation(disconnectedHandler);
    });

    it('shows "Disconnected" text', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const indicator = wrapper.find('.status-indicator');
      expect(indicator.text()).toContain('Disconnected');
    });

    it('shows grey dot (no .active or .connected class)', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const dot = wrapper.find('.dot');
      expect(dot.classes()).not.toContain('active');
      expect(dot.classes()).not.toContain('connected');
    });

    it('disables the Enable Enhancement checkbox', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const checkbox = wrapper.find('input[type="checkbox"]');
      expect(checkbox.element.disabled).toBe(true);
    });

    it('adds disabled class to controls section', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const controls = wrapper.find('.controls');
      expect(controls.classes()).toContain('disabled');
    });

    it('adds disabled class to switch', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const switchLabel = wrapper.find('.switch');
      expect(switchLabel.classes()).toContain('disabled');
    });

    it('passes disabled prop to all SliderControls', async () => {
      const wrapper = mount(App, {
        global: { stubs: {} },  // Don't stub — need real component
      });
      await flushPromises();
      await triggerPoll(wrapper);
      const sliders = wrapper.findAll('input[type="range"]');
      sliders.forEach(slider => {
        expect(slider.element.disabled).toBe(true);
      });
    });

    it('hides status-info bar (cutoff/CPU/DSP)', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.status-info').exists()).toBe(false);
    });

    it('shows Install APO button (not Uninstall)', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.btn.install').exists()).toBe(true);
      expect(wrapper.find('.btn.uninstall').exists()).toBe(false);
    });
  });

  // =========================================================================
  // STATE 2: Standby (APO connected, no audio flowing)
  // =========================================================================
  describe('Standby state', () => {
    beforeEach(() => {
      mockInvoke.mockImplementation(standbyHandler);
    });

    it('shows "Standby" text', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const indicator = wrapper.find('.status-indicator');
      expect(indicator.text()).toContain('Standby');
    });

    it('shows yellow dot (.connected class, no .active)', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const dot = wrapper.find('.dot');
      expect(dot.classes()).toContain('connected');
      expect(dot.classes()).not.toContain('active');
    });

    it('enables the Enable Enhancement checkbox', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const checkbox = wrapper.find('input[type="checkbox"]');
      expect(checkbox.element.disabled).toBe(false);
    });

    it('does NOT add disabled class to controls', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const controls = wrapper.find('.controls');
      expect(controls.classes()).not.toContain('disabled');
    });

    it('shows status-info bar', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.status-info').exists()).toBe(true);
    });

    it('shows Uninstall APO button', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.btn.uninstall').exists()).toBe(true);
      expect(wrapper.find('.btn.install').exists()).toBe(false);
    });
  });

  // =========================================================================
  // STATE 3: Processing (APO connected, audio flowing)
  // =========================================================================
  describe('Processing state', () => {
    beforeEach(() => {
      mockInvoke.mockImplementation(processingHandler);
    });

    it('shows "Processing" text', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const indicator = wrapper.find('.status-indicator');
      expect(indicator.text()).toContain('Processing');
    });

    it('shows green dot (.active class)', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const dot = wrapper.find('.dot');
      expect(dot.classes()).toContain('active');
    });

    it('adds .active class to status-bar', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const bar = wrapper.find('.status-bar');
      expect(bar.classes()).toContain('active');
    });

    it('enables all controls', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const checkbox = wrapper.find('input[type="checkbox"]');
      expect(checkbox.element.disabled).toBe(false);
      expect(wrapper.find('.controls').classes()).not.toContain('disabled');
    });

    it('shows DSP active indicator with dB value', async () => {
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const algoCell = wrapper.find('.algo-active');
      expect(algoCell.exists()).toBe(true);
      expect(algoCell.text()).toContain('DSP');
      expect(algoCell.text()).toContain('dB');
    });
  });

  // =========================================================================
  // TRANSITIONS
  // =========================================================================
  describe('State transitions', () => {
    it('Disconnected → Standby: enables controls when APO connects', async () => {
      // Start disconnected
      mockInvoke.mockImplementation(disconnectedHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.controls').classes()).toContain('disabled');
      expect(wrapper.find('.status-indicator').text()).toContain('Disconnected');

      // APO connects → standby
      mockInvoke.mockImplementation(standbyHandler);
      await triggerPoll(wrapper);
      expect(wrapper.find('.controls').classes()).not.toContain('disabled');
      expect(wrapper.find('.status-indicator').text()).toContain('Standby');
    });

    it('Standby → Processing: dot changes to green when audio flows', async () => {
      // Start standby
      mockInvoke.mockImplementation(standbyHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.dot').classes()).toContain('connected');
      expect(wrapper.find('.dot').classes()).not.toContain('active');

      // Audio starts
      mockInvoke.mockImplementation(processingHandler);
      await triggerPoll(wrapper);
      expect(wrapper.find('.dot').classes()).toContain('active');
    });

    it('Processing → Disconnected: resets enabled to false and disables controls', async () => {
      // Start processing
      mockInvoke.mockImplementation(processingHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('input[type="checkbox"]').element.disabled).toBe(false);

      // APO disconnects
      mockInvoke.mockImplementation(disconnectedHandler);
      await triggerPoll(wrapper);

      // enabled should reset to false
      const checkbox = wrapper.find('input[type="checkbox"]');
      expect(checkbox.element.checked).toBe(false);
      expect(checkbox.element.disabled).toBe(true);
      expect(wrapper.find('.controls').classes()).toContain('disabled');
      expect(wrapper.find('.status-indicator').text()).toContain('Disconnected');
    });

    it('Standby → Disconnected: resets enabled to false', async () => {
      // Start standby
      mockInvoke.mockImplementation(standbyHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);

      // APO disconnects
      mockInvoke.mockImplementation(disconnectedHandler);
      await triggerPoll(wrapper);

      const checkbox = wrapper.find('input[type="checkbox"]');
      expect(checkbox.element.checked).toBe(false);
      expect(checkbox.element.disabled).toBe(true);
    });

    it('Disconnected → Processing: full transition when APO starts with audio', async () => {
      mockInvoke.mockImplementation(disconnectedHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.status-indicator').text()).toContain('Disconnected');

      mockInvoke.mockImplementation(processingHandler);
      await triggerPoll(wrapper);
      expect(wrapper.find('.status-indicator').text()).toContain('Processing');
      expect(wrapper.find('.controls').classes()).not.toContain('disabled');
    });
  });

  // =========================================================================
  // UNINSTALL FLOW
  // =========================================================================
  describe('Uninstall flow', () => {
    it('after uninstall: shows Disconnected + disabled + Install button', async () => {
      // Start with APO installed and standby
      mockInvoke.mockImplementation(standbyHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.btn.uninstall').exists()).toBe(true);

      // Simulate uninstall — status becomes null, apoInstalled becomes false
      mockInvoke.mockImplementation((cmd) => {
        if (cmd === 'get_status') return Promise.resolve(null);
        if (cmd === 'is_apo_installed') return Promise.resolve(false);
        return disconnectedHandler(cmd);
      });
      await triggerPoll(wrapper);

      expect(wrapper.find('.status-indicator').text()).toContain('Disconnected');
      expect(wrapper.find('.controls').classes()).toContain('disabled');
      expect(wrapper.find('input[type="checkbox"]').element.disabled).toBe(true);
      expect(wrapper.find('input[type="checkbox"]').element.checked).toBe(false);
      expect(wrapper.find('.btn.install').exists()).toBe(true);
      expect(wrapper.find('.btn.uninstall').exists()).toBe(false);
    });
  });

  // =========================================================================
  // SMOOTHED DISPLAY VALUES
  // =========================================================================
  describe('Smoothed display values', () => {
    it('clears smoothed values on disconnect', async () => {
      // Start processing with visible stats
      mockInvoke.mockImplementation(processingHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      expect(wrapper.find('.status-info').exists()).toBe(true);

      // Disconnect
      mockInvoke.mockImplementation(disconnectedHandler);
      await triggerPoll(wrapper);

      // status-info should be hidden (v-if="status")
      expect(wrapper.find('.status-info').exists()).toBe(false);
    });

    it('shows CPU percentage in status-info when connected', async () => {
      mockInvoke.mockImplementation(processingHandler);
      const wrapper = await mountApp();
      await triggerPoll(wrapper);
      const text = wrapper.find('.status-info').text();
      expect(text).toContain('CPU');
      expect(text).toContain('%');
    });
  });
});
