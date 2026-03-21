# 30 — License Interface Specification

## Purpose

This document specifies the license system architecture for Phaselith. It is designed to be read by another development session for full implementation.

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│  Open-Source Repo (AGPL)                            │
│                                                     │
│  crates/license/     ← trait + FreeLicense          │
│  crates/dsp-core/    ← pure DSP, license-unaware    │
│  crates/apo-dll/     ← Windows APO                  │
│  crates/tauri-app/   ← Tauri + Vue UI               │
│  crates/wasm-bridge/ ← Chrome WASM bridge            │
│  crates/core-audio/  ← macOS (future)               │
│  chrome-ext/         ← Chrome extension JS           │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│  Closed-Source Repo (Private)                       │
│                                                     │
│  crates/license-pro/  ← ProLicenseProvider impls    │
│    ├─ chrome.rs       ← Chrome Web Store payments   │
│    ├─ apo.rs          ← Windows license file/server │
│    ├─ core_audio.rs   ← macOS App Store / receipt   │
│    └─ vst3.rs         ← iLok / serial key           │
│  server/              ← License validation API      │
└─────────────────────────────────────────────────────┘
```

## Core Principle

**DSP core never knows about licensing.** The enforcement happens at every config write boundary — each platform clamps `EngineConfig` before passing it to the engine.

## Trait Interface

Located at `crates/license/src/lib.rs` (already implemented):

```rust
pub enum Platform {
    ChromeExtension,
    WindowsApo,
    CoreAudio,
    Vst3,
}

pub enum LicenseTier {
    Free,
    Pro,
}

pub trait LicenseProvider: Send + Sync {
    fn platform(&self) -> Platform;
    fn tier(&self) -> LicenseTier;
    fn max_strength(&self) -> f32;          // Free=0.5, Pro=1.0
    fn available_presets(&self) -> &[FilterStyle]; // Free=[Reference], Pro=all
}

/// Clamp config before passing to DSP engine
pub fn clamp_config(config: &mut EngineConfig, license: &dyn LicenseProvider);
```

`FreeLicense` is the default implementation (always Free tier). Ships with the open-source repo.

## Free vs Pro Feature Matrix

| Feature | Free | Pro |
|---------|------|-----|
| Strength | 0–50% | 0–100% |
| Filter Style | Reference only | Reference, Warm, Bass+, Custom |
| HF Reconstruction | Full | Full |
| Dynamics | Full | Full |
| Transient | Full | Full |
| Advanced 6-axis | Locked | Unlocked |

## Platform Integration Points

### Windows APO (via Tauri)

**File:** `crates/tauri-app/src/commands.rs`

1. Hold `Box<dyn LicenseProvider>` in Tauri app state
2. Default: `FreeLicense::new(Platform::WindowsApo)`
3. In `set_config()` command: call `clamp_config(&mut config, &*license)` before writing to mmap
4. Add `get_license_info` command:

```rust
#[derive(Serialize)]
pub struct LicenseInfo {
    pub tier: String,          // "Free" or "Pro"
    pub platform: String,
    pub max_strength: f32,
    pub available_presets: Vec<String>,
}

#[tauri::command]
pub fn get_license_info(state: State<AppState>) -> LicenseInfo {
    let license = &state.license;
    LicenseInfo {
        tier: format!("{:?}", license.tier()),
        platform: format!("{:?}", license.platform()),
        max_strength: license.max_strength(),
        available_presets: license.available_presets()
            .iter().map(|p| format!("{:?}", p)).collect(),
    }
}
```

### Chrome Extension

**Files:** `chrome-ext/popup.js`, `chrome-ext/worklet-processor.js`

License enforcement is in JavaScript (not Rust):
1. Read license state from `chrome.storage.local`
2. `popup.js`: cap strength slider max, lock preset buttons, show upgrade prompt
3. `worklet-processor.js`: defense-in-depth `this.strength = Math.min(this.strength, maxStrength)`
4. Payment integration via Chrome Web Store Payments API or ExtensionPay (closed-source overlay)

### VST3 (Future)

The `nih-plug` wrapper holds a `Box<dyn LicenseProvider>` and clamps parameters in `set_param_normalized()`. License validation via iLok or serial key (closed-source).

### macOS Core Audio (Future)

Same Tauri pattern as Windows. Companion app holds license provider, clamps config before writing to shared memory.

## Tauri Vue UI Changes

### File: `crates/tauri-app/frontend/src/App.vue`

#### 1. License State (reactive)

```javascript
const license = ref({ tier: 'Free', max_strength: 0.5, available_presets: ['Reference'] });

onMounted(async () => {
    license.value = await invoke('get_license_info');
});
```

#### 2. Tier Badge (header)

Add next to "Phaselith" title:

```html
<span class="tier-badge" :class="license.tier.toLowerCase()">
  {{ license.tier }}
</span>
```

CSS:
```css
.tier-badge.free { background: #333; color: #999; }
.tier-badge.pro { background: #00d4ff22; color: #00d4ff; border: 1px solid #00d4ff; }
```

#### 3. Strength Slider Capping

Pass max to SliderControl:

```html
<SliderControl
  label="Compensation Strength"
  v-model="config.strength"
  :max="license.max_strength * 100"
  :disabled="!apoInstalled"
  @update:modelValue="applyConfig"
/>
```

`SliderControl.vue` needs a `max` prop (default 100).

#### 4. Preset Locking

Filter style pills show lock icon for unavailable presets:

```html
<button
  v-for="(style, idx) in filterStyles"
  :key="idx"
  class="pill"
  :class="{
    active: config.filter_style === idx,
    locked: !license.available_presets.includes(style)
  }"
  @click="selectOrUpgrade(idx, style)"
>{{ style }}</button>
```

```javascript
function selectOrUpgrade(idx, style) {
    if (license.value.available_presets.includes(style)) {
        selectPreset(idx);
    } else {
        showUpgradePrompt.value = true;
    }
}
```

#### 5. Upgrade Prompt Modal

New component `UpgradePrompt.vue`:

```html
<div class="upgrade-overlay" v-if="visible" @click.self="$emit('close')">
  <div class="upgrade-card">
    <h3>Unlock Phaselith Pro</h3>
    <p>Full strength control, all style presets, and advanced features.</p>
    <p class="price">$4.99/month or $39.99/year</p>
    <p class="tagline">☕ Less than a cup of coffee per month.</p>
    <button class="btn upgrade" @click="openPurchase">Upgrade</button>
    <button class="btn cancel" @click="$emit('close')">Maybe later</button>
  </div>
</div>
```

#### 6. Advanced 6-axis Locking

For Free tier, the "Advanced Style Axes" section is locked:

```html
<div class="advanced-toggle" @click="toggleAdvanced">
  <span class="chevron" :class="{ open: advancedOpen }">▸</span>
  <label>Advanced Style Axes</label>
  <span v-if="license.tier === 'Free'" class="lock-icon">🔒</span>
</div>
```

## Cargo Feature Flag for Pro Build

```toml
# crates/tauri-app/Cargo.toml
[features]
default = []
pro-license = ["phaselith-license-pro"]

[dependencies]
phaselith-license = { path = "../license" }
phaselith-license-pro = { path = "../license-pro", optional = true }
```

Build commands:
- Open-source / Free: `cargo build`
- Pro release: `cargo build --features pro-license`

## Implementation Order

### Phase 1: License Crate ✅ DONE
- [x] `crates/license/` with trait, FreeLicense, clamp_config
- [x] Added to workspace members
- [x] Unit tests

### Phase 2: Tauri Backend Integration
- [ ] Add `phaselith-license` dependency to `crates/tauri-app/Cargo.toml`
- [ ] Hold `Box<dyn LicenseProvider>` in app state
- [ ] Add `get_license_info` Tauri command
- [ ] Call `clamp_config()` in `set_config()` before mmap write

### Phase 3: Vue UI — License Display & Gating
- [ ] Add `max` prop to `SliderControl.vue`
- [ ] Fetch license info on mount
- [ ] Tier badge in header
- [ ] Strength slider capping
- [ ] Preset lock icons
- [ ] Upgrade prompt modal
- [ ] Advanced axes locking

### Phase 4: Chrome Extension — JS Enforcement
- [ ] License state in `chrome.storage.local`
- [ ] Popup UI gating (slider cap, preset locks, upgrade prompt)
- [ ] Worklet defense-in-depth clamp
- [ ] Payment integration (closed-source)

### Phase 5: Future Platforms
- [ ] VST3 wrapper with license clamping
- [ ] CoreAudio companion app
- [ ] Pro license provider crate (closed-source repo)
- [ ] License validation server

## Security Notes

1. **Client-side enforcement is not DRM.** A determined user can patch the binary. This is acceptable — the goal is honest gating for honest users, not anti-piracy.
2. **Defense-in-depth:** Clamp at UI layer AND at config write layer. Even if someone modifies the UI, the backend clamp catches it.
3. **No license logic in DSP core.** The core is open-source and must remain license-unaware. All enforcement is in the platform layer.
