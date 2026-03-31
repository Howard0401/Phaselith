/// Synthesis mode selector — controls how M5 converts validated
/// freq-domain residual to time-domain output.
///
/// Lives in config (not frame.rs) so it can be changed at runtime from the UI.
/// LegacyAdditive preserves the existing sonic identity.
/// FftOlaPilot enables the new ISTFT+OLA path for A/B comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthesisMode {
    /// Current additive synthesis path (sum of cosines).
    LegacyAdditive,
    /// Pilot: core-lattice only ISTFT + OLA.
    FftOlaPilot,
    /// Full hop-aligned ISTFT + OLA across all lattices.
    FftOlaFull,
}

impl Default for SynthesisMode {
    fn default() -> Self {
        SynthesisMode::LegacyAdditive
    }
}

impl SynthesisMode {
    /// Convert from integer (for WASM/UI bridge).
    /// 0=LegacyAdditive, 1=FftOlaPilot, 2=FftOlaFull.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => SynthesisMode::FftOlaPilot,
            2 => SynthesisMode::FftOlaFull,
            _ => SynthesisMode::LegacyAdditive,
        }
    }

    /// Convert to integer for serialization.
    pub fn to_u32(self) -> u32 {
        match self {
            SynthesisMode::LegacyAdditive => 0,
            SynthesisMode::FftOlaPilot => 1,
            SynthesisMode::FftOlaFull => 2,
        }
    }
}

/// Phaselith engine configuration.
/// Controlled by the UI (Tauri or Chrome popup) or shared memory (APO).
/// All fields are plain data — safe to copy across thread boundaries.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    /// Overall compensation strength (0.0 = bypass, 1.0 = full).
    pub strength: f32,
    /// High-frequency reconstruction intensity.
    pub hf_reconstruction: f32,
    /// Dynamic range restoration intensity.
    pub dynamics: f32,
    /// Transient repair intensity.
    pub transient: f32,
    /// Scales how strongly the transient control affects pre-echo suppression.
    /// 1.0 keeps the original coupling, 0.0 disables the pre-echo path while
    /// leaving other transient-driven stages untouched.
    pub pre_echo_transient_scaling: f32,
    /// Scales how strongly the transient control affects declip peak estimation.
    /// 1.0 keeps the original coupling, 0.0 forces the conservative declip path.
    pub declip_transient_scaling: f32,
    /// When true, pre-echo suppression is applied on a one-block delayed
    /// output path instead of mutating the current host callback block.
    /// Used by browser runtimes that need frame-aligned transient shaping.
    pub delayed_transient_repair: bool,
    /// Experimental browser A/B toggle that re-opens the low-mid
    /// body lane (180-500 Hz) below the cutoff when harmonic structure exists.
    pub body_pass_enabled: bool,
    /// Extra high-frequency taming on top of the default M6 de-emphasis shelf.
    /// 0.0 = current behavior, 1.0 = maximum extra harshness reduction.
    pub hf_tame: f32,
    /// Low-end elasticity control for the M5 impact/body window.
    /// 0.0 = current behavior, 1.0 = widest / softest bass flex path.
    pub bass_flex: f32,
    /// Phase correction mode.
    pub phase_mode: PhaseMode,
    /// Quality mode (affects CPU/GPU budget).
    pub quality_mode: QualityMode,
    /// Global enable/disable.
    pub enabled: bool,
    /// Style / character preset.
    pub style: StyleConfig,
    /// Synthesis mode — controls freq→time conversion path in M5.
    pub synthesis_mode: SynthesisMode,
    /// Ambience preserve: compensates the dereverb side-effect of M5 reprojection.
    /// 0.0 = no compensation (default), 1.0 = full tail preservation.
    /// Independent parameter — does NOT reuse spatial_spread.
    /// Start very low (0.05-0.15) and tune by ear.
    pub ambience_preserve: f32,
    /// Low-mid glue for ambience preserve source shaping.
    /// 0.0 = current behavior, 1.0 = maximum room/body reconnection.
    pub ambience_glue: f32,
    /// High-level filter style selector.
    /// When changed, overrides the 6-axis StyleConfig with the preset values.
    pub filter_style: FilterStyle,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            strength: 0.7,
            hf_reconstruction: 0.8,
            dynamics: 0.6,
            transient: 0.5,
            pre_echo_transient_scaling: 1.0,
            declip_transient_scaling: 1.0,
            delayed_transient_repair: false,
            body_pass_enabled: false,
            hf_tame: 0.0,
            bass_flex: 0.0,
            phase_mode: PhaseMode::Linear,
            quality_mode: QualityMode::Standard,
            enabled: true,
            style: StyleConfig::default(),
            synthesis_mode: SynthesisMode::default(),
            ambience_preserve: 0.0,
            ambience_glue: 0.0,
            filter_style: FilterStyle::default(),
        }
    }
}

// ─── Style / Character System ───

/// Style preset identifiers (legacy fine-grained presets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StylePreset {
    Reference,
    Grand,
    Smooth,
    Vocal,
    Punch,
    Air,
    Night,
}

/// High-level filter style — user-facing audio character selector.
///
/// Three preset sonic identities plus a manual Custom mode:
/// - Reference: faithful restoration, minimal coloration
/// - Warm: tube-like character, even harmonics, gentle HF rolloff
/// - BassPlus: enhanced low-end, sub-harmonic synthesis, impact boost
/// - Custom: user-defined 6-axis StyleConfig (debug/advanced UI)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterStyle {
    /// Faithful restoration — current default. Prioritizes accuracy.
    Reference,
    /// Tube analog warmth — even-harmonic saturation, softer highs,
    /// richer mid-range. Inspired by 12AX7 transfer curve.
    Warm,
    /// Bass enhancement — sub-harmonic reinforcement, impact boost,
    /// body enrichment. For headphones that lack low-end.
    BassPlus,
    /// User-defined: 6-axis StyleConfig set manually via debug UI.
    /// When active, APO reads individual axis values from mmap instead
    /// of deriving from a preset.
    Custom,
}

impl Default for FilterStyle {
    fn default() -> Self {
        FilterStyle::Reference
    }
}

impl FilterStyle {
    /// Convert from integer (for WASM/UI/mmap bridge).
    /// 0=Reference, 1=Warm, 2=BassPlus, 3=Custom.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => FilterStyle::Warm,
            2 => FilterStyle::BassPlus,
            3 => FilterStyle::Custom,
            _ => FilterStyle::Reference,
        }
    }

    /// Convert to integer for serialization.
    pub fn to_u32(self) -> u32 {
        match self {
            FilterStyle::Reference => 0,
            FilterStyle::Warm => 1,
            FilterStyle::BassPlus => 2,
            FilterStyle::Custom => 3,
        }
    }

    /// Get the StyleConfig axes for this filter style.
    /// Custom returns Reference defaults — caller should override with actual values.
    pub fn to_style_config(self) -> StyleConfig {
        match self {
            //                                warmth  air_br  smooth  spatial impact  body
            FilterStyle::Reference => StyleConfig::new(0.15, 0.50, 0.40, 0.30, 0.15, 0.40),
            FilterStyle::Warm =>      StyleConfig::new(0.55, 0.30, 0.60, 0.30, 0.15, 0.50),
            FilterStyle::BassPlus =>  StyleConfig::new(0.20, 0.45, 0.35, 0.30, 0.45, 0.75),
            FilterStyle::Custom =>    StyleConfig::new(0.15, 0.50, 0.40, 0.30, 0.15, 0.40),
        }
    }

    /// Returns true if this is a preset (not Custom).
    pub fn is_preset(self) -> bool {
        !matches!(self, FilterStyle::Custom)
    }

    /// Display name for UI.
    pub fn display_name(self) -> &'static str {
        match self {
            FilterStyle::Reference => "Reference",
            FilterStyle::Warm => "Warm",
            FilterStyle::BassPlus => "Bass+",
            FilterStyle::Custom => "Custom",
        }
    }
}

/// Six-axis character configuration.
/// These parameters drive the "character layer" that operates
/// independently of damage-driven restoration, ensuring every
/// song — even high-quality sources — gets a perceptible upgrade.
#[derive(Debug, Clone, Copy)]
pub struct StyleConfig {
    /// Subtle even-harmonic saturation (tube-like warmth). 0.0-1.0
    pub warmth: f32,
    /// HF extension slope multiplier (darker ↔ brighter). 0.0-1.0
    pub air_brightness: f32,
    /// Upper-mid roughness suppression. 0.0-1.0
    pub smoothness: f32,
    /// Stereo side recovery aggressiveness. 0.0-1.0
    pub spatial_spread: f32,
    /// Impact band (80-180 Hz) opening for transient punch. 0.0-1.0
    pub impact_gain: f32,
    /// Low-mid harmonic body reinforcement. 0.0-1.0
    pub body: f32,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self::from_preset(StylePreset::Reference)
    }
}

impl StyleConfig {
    /// Create a StyleConfig from a named preset.
    pub fn from_preset(preset: StylePreset) -> Self {
        match preset {
            //                  warmth  air_br  smooth  spatial impact  body
            StylePreset::Reference => Self::new(0.15, 0.50, 0.40, 0.30, 0.15, 0.40),
            StylePreset::Grand => Self::new(0.25, 0.80, 0.50, 0.45, 0.18, 0.35),
            StylePreset::Smooth => Self::new(0.20, 0.30, 0.75, 0.25, 0.12, 0.50),
            StylePreset::Vocal => Self::new(0.18, 0.40, 0.45, 0.20, 0.20, 0.55),
            StylePreset::Punch => Self::new(0.20, 0.45, 0.35, 0.30, 0.35, 0.60),
            StylePreset::Air => Self::new(0.10, 0.90, 0.35, 0.40, 0.10, 0.30),
            StylePreset::Night => Self::new(0.30, 0.20, 0.80, 0.20, 0.10, 0.55),
        }
    }

    /// Construct with explicit values.
    pub fn new(
        warmth: f32,
        air_brightness: f32,
        smoothness: f32,
        spatial_spread: f32,
        impact_gain: f32,
        body: f32,
    ) -> Self {
        Self {
            warmth,
            air_brightness,
            smoothness,
            spatial_spread,
            impact_gain,
            body,
        }
    }

    /// Overall character intensity (average of all axes).
    /// Used to compute the character floor in M6.
    pub fn character_intensity(&self) -> f32 {
        (self.warmth
            + self.air_brightness
            + self.smoothness
            + self.spatial_spread
            + self.impact_gain
            + self.body)
            / 6.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseMode {
    /// Better quality, higher latency.
    Linear,
    /// Lower latency, slight phase artifacts.
    Minimum,
}

/// Quality mode controlling CPU/GPU budget and algorithm complexity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityMode {
    /// CPU-only, M2-M5 simplified, ~6-10ms latency.
    Light,
    /// CPU+GPU (if available), standard precision, ~14-24ms latency.
    Standard,
    /// Full pipeline, maximum quality, ~28-45ms latency.
    Ultra,
    /// Maximum FFT (4096), 5 reprojection iterations, ~50-85ms latency.
    /// Uses 2x the CPU of Ultra for finer spectral resolution and
    /// more accurate residual validation. For systems with headroom.
    Extreme,
    /// FFT 8192, 8 reprojection iterations, ~100-170ms latency.
    /// Doubles Extreme's spectral resolution and adds 3 more reprojection
    /// passes. For A/B comparison against Extreme on high-end CPUs.
    UltraExtreme,
}

impl QualityMode {
    /// Core lattice FFT size for this quality mode.
    pub fn core_fft_size(&self) -> usize {
        match self {
            QualityMode::Light => 512,
            QualityMode::Standard => 1024,
            QualityMode::Ultra => 2048,
            QualityMode::Extreme => 4096,
            QualityMode::UltraExtreme => 8192,
        }
    }

    /// Hop size (core lattice) for this quality mode.
    /// Aligned to factors of 480 (Windows APO frame size @ 48kHz)
    /// so STFT analysis boundaries divide evenly into the host frame.
    pub fn hop_size(&self) -> usize {
        match self {
            QualityMode::Light => 128,       // 512/4=128, COLA-safe with Hann
            QualityMode::Standard => 128,    // 1024/8=128, COLA-safe with Hann
            QualityMode::Ultra => 128,       // 2048/16=128, COLA-safe with Hann
            QualityMode::Extreme => 128,     // 4096/32=128, COLA-safe with Hann
            QualityMode::UltraExtreme => 128,// 8192/64=128, COLA-safe with Hann
        }
    }

    /// Maximum reprojection iterations allowed.
    pub fn max_reprojection_iters(&self) -> usize {
        match self {
            QualityMode::Light => 1,
            QualityMode::Standard => 2,
            QualityMode::Ultra => 3,
            QualityMode::Extreme => 5,
            QualityMode::UltraExtreme => 8,
        }
    }
}

// ─── Legacy compatibility ───

impl QualityMode {
    /// Legacy FFT size method (used by old StageContext).
    pub fn fft_size(&self) -> usize {
        self.core_fft_size()
    }
}

pub type DspConfig = EngineConfig;
pub type QualityPreset = QualityMode;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_enabled() {
        let config = EngineConfig::default();
        assert!(config.enabled);
        assert_eq!(config.quality_mode, QualityMode::Standard);
    }

    #[test]
    fn quality_mode_fft_sizes() {
        assert_eq!(QualityMode::Light.core_fft_size(), 512);
        assert_eq!(QualityMode::Standard.core_fft_size(), 1024);
        assert_eq!(QualityMode::Ultra.core_fft_size(), 2048);
    }

    #[test]
    fn quality_mode_hop_sizes() {
        assert_eq!(QualityMode::Light.hop_size(), 128);
        assert_eq!(QualityMode::Standard.hop_size(), 128);
        assert_eq!(QualityMode::Ultra.hop_size(), 128);
    }

    #[test]
    fn quality_mode_reprojection_iters() {
        assert_eq!(QualityMode::Light.max_reprojection_iters(), 1);
        assert_eq!(QualityMode::Standard.max_reprojection_iters(), 2);
        assert_eq!(QualityMode::Ultra.max_reprojection_iters(), 3);
    }
}
