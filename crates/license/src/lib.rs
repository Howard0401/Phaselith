//! Phaselith License Interface
//!
//! Defines the platform-agnostic license trait. Each platform (Chrome, APO,
//! Core Audio, VST3) provides its own implementation. The open-source repo
//! ships only `FreeLicense`; Pro validation lives in a separate closed-source crate.

use phaselith_dsp_core::config::{EngineConfig, FilterStyle};

// ─── Platform ───

/// Target platform for license enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    ChromeExtension,
    WindowsApo,
    CoreAudio,
    Vst3,
}

// ─── Tier ───

/// License tier determines feature access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LicenseTier {
    Free,
    Pro,
}

// ─── Trait ───

/// License provider interface.
///
/// Each platform implements this trait with its own validation logic.
/// The DSP core never depends on this — callers clamp `EngineConfig`
/// before passing it to the engine.
pub trait LicenseProvider: Send + Sync {
    /// Which platform this provider serves.
    fn platform(&self) -> Platform;

    /// Current license tier.
    fn tier(&self) -> LicenseTier;

    /// Maximum allowed strength (0.0–1.0). Free = 0.5, Pro = 1.0.
    fn max_strength(&self) -> f32;

    /// Available filter style presets for the current tier.
    fn available_presets(&self) -> &[FilterStyle];
}

// ─── Free (default) ───

/// Default license: always Free tier.
/// Ships with the open-source repo. No validation, no network calls.
pub struct FreeLicense {
    platform: Platform,
}

impl FreeLicense {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }
}

static FREE_PRESETS: [FilterStyle; 1] = [FilterStyle::Reference];

impl LicenseProvider for FreeLicense {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn tier(&self) -> LicenseTier {
        LicenseTier::Free
    }

    fn max_strength(&self) -> f32 {
        0.5
    }

    fn available_presets(&self) -> &[FilterStyle] {
        &FREE_PRESETS
    }
}

// ─── Config clamping ───

/// Clamp an `EngineConfig` to respect license limits.
/// Called by the platform layer BEFORE passing config to the engine.
pub fn clamp_config(config: &mut EngineConfig, license: &dyn LicenseProvider) {
    config.strength = config.strength.min(license.max_strength());

    let presets = license.available_presets();
    if !presets.contains(&config.filter_style) {
        config.filter_style = presets[0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_license_caps_strength() {
        let license = FreeLicense::new(Platform::WindowsApo);
        assert_eq!(license.tier(), LicenseTier::Free);
        assert_eq!(license.max_strength(), 0.5);

        let mut config = EngineConfig::default();
        config.strength = 0.9;
        clamp_config(&mut config, &license);
        assert!((config.strength - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn free_license_forces_reference_preset() {
        let license = FreeLicense::new(Platform::ChromeExtension);
        let mut config = EngineConfig::default();
        config.filter_style = FilterStyle::Warm;
        clamp_config(&mut config, &license);
        assert_eq!(config.filter_style, FilterStyle::Reference);
    }

    #[test]
    fn free_license_allows_reference() {
        let license = FreeLicense::new(Platform::CoreAudio);
        let mut config = EngineConfig::default();
        config.filter_style = FilterStyle::Reference;
        config.strength = 0.3;
        clamp_config(&mut config, &license);
        assert_eq!(config.filter_style, FilterStyle::Reference);
        assert!((config.strength - 0.3).abs() < f32::EPSILON);
    }
}
