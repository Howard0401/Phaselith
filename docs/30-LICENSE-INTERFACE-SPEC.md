# 30 — License Interface Specification

## Purpose

Defines the license trait interface for Phaselith. Platform-specific implementations are maintained separately.

## Core Principle

**DSP core never knows about licensing.** Enforcement happens at every config write boundary — each platform clamps `EngineConfig` before passing it to the engine.

## Trait Interface

Located at `crates/license/src/lib.rs`:

```rust
pub enum Platform { ChromeExtension, WindowsApo, CoreAudio, Vst3 }
pub enum LicenseTier { Free, Pro }

pub trait LicenseProvider: Send + Sync {
    fn platform(&self) -> Platform;
    fn tier(&self) -> LicenseTier;
    fn max_strength(&self) -> f32;
    fn available_presets(&self) -> &[FilterStyle];
}

pub struct FreeLicense { ... }  // Default: always Free
pub fn clamp_config(config: &mut EngineConfig, license: &dyn LicenseProvider);
```

## Free Tier Limits

| Feature | Free | Pro |
|---------|------|-----|
| Strength | 0–50% | 0–100% |
| Filter Style | Reference only | All |
| Advanced 6-axis | Locked | Unlocked |
| HF / Dynamics / Transient | Full | Full |

## Integration Pattern

Each platform holds a `Box<dyn LicenseProvider>` and calls `clamp_config()` before passing config to the DSP engine. The open-source repo ships `FreeLicense` as the default. Pro implementations are maintained separately.

## Cargo Feature Flag

```toml
[features]
default = []
pro-license = ["phaselith-license-pro"]
```

Build: `cargo build` → Free. `cargo build --features pro-license` → Pro.
