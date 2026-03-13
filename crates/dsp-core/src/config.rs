/// CIRRUS engine configuration.
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
    /// Phase correction mode.
    pub phase_mode: PhaseMode,
    /// Quality mode (affects CPU/GPU budget).
    pub quality_mode: QualityMode,
    /// Global enable/disable.
    pub enabled: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            strength: 0.7,
            hf_reconstruction: 0.8,
            dynamics: 0.6,
            transient: 0.5,
            phase_mode: PhaseMode::Linear,
            quality_mode: QualityMode::Standard,
            enabled: true,
        }
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
}

impl QualityMode {
    /// Core lattice FFT size for this quality mode.
    pub fn core_fft_size(&self) -> usize {
        match self {
            QualityMode::Light => 512,
            QualityMode::Standard => 1024,
            QualityMode::Ultra => 2048,
        }
    }

    /// Hop size (core lattice) for this quality mode.
    pub fn hop_size(&self) -> usize {
        self.core_fft_size() / 4
    }

    /// Maximum reprojection iterations allowed.
    pub fn max_reprojection_iters(&self) -> usize {
        match self {
            QualityMode::Light => 1,
            QualityMode::Standard => 2,
            QualityMode::Ultra => 3,
        }
    }
}

// ─── Legacy compatibility ───

impl QualityMode {
    /// Legacy FFT size method (used by old StageContext).
    pub fn fft_size(&self) -> usize { self.core_fft_size() }
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
        assert_eq!(QualityMode::Standard.hop_size(), 256);
        assert_eq!(QualityMode::Ultra.hop_size(), 512);
    }

    #[test]
    fn quality_mode_reprojection_iters() {
        assert_eq!(QualityMode::Light.max_reprojection_iters(), 1);
        assert_eq!(QualityMode::Standard.max_reprojection_iters(), 2);
        assert_eq!(QualityMode::Ultra.max_reprojection_iters(), 3);
    }
}
