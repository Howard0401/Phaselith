mod cutoff;
mod clipping;
mod compression;
mod stereo;

pub use self::cutoff::detect_cutoff;
pub use self::clipping::detect_clipping;
pub use self::compression::estimate_compression;
pub use self::stereo::analyze_stereo;

use crate::stages::stage_trait::{DspStage, StageContext};
use crate::types::{DegradationProfile, QualityTier};

/// Stage 0: Degradation Fingerprint Detection.
///
/// Analyzes audio to identify what kind of damage it has suffered:
/// - High-frequency cutoff (lossy codec truncation)
/// - Clipping (mastering/mixing overload)
/// - Dynamic compression (loudness war)
/// - Stereo degradation (joint stereo artifacts)
///
/// Runs only every N frames (~0.3s) since source quality is stable within a track.
pub struct FingerprintDetector {
    frame_counter: u64,
    update_interval: u64,
    /// Rolling history of cutoff detections for stability.
    cutoff_history: [f32; 8],
    cutoff_history_idx: usize,
    /// Cached magnitude buffer (pre-allocated).
    magnitude_buf: Vec<f32>,
}

impl FingerprintDetector {
    pub fn new() -> Self {
        Self {
            frame_counter: 0,
            update_interval: 32,
            cutoff_history: [20000.0; 8],
            cutoff_history_idx: 0,
            magnitude_buf: Vec::new(),
        }
    }

    fn classify_quality(cutoff: Option<f32>) -> QualityTier {
        match cutoff {
            None => QualityTier::Lossless,
            Some(f) if f > 19500.0 => QualityTier::Lossless,
            Some(f) if f > 17000.0 => QualityTier::High,
            Some(f) if f > 15000.0 => QualityTier::Medium,
            Some(_) => QualityTier::Low,
        }
    }

    fn cutoff_stability(&self) -> f32 {
        if self.cutoff_history.len() < 2 {
            return 0.5;
        }
        let mean: f32 = self.cutoff_history.iter().sum::<f32>() / self.cutoff_history.len() as f32;
        let variance: f32 = self.cutoff_history.iter()
            .map(|&x| (x - mean) * (x - mean))
            .sum::<f32>() / self.cutoff_history.len() as f32;
        let std_dev = variance.sqrt();
        // Low std_dev relative to mean = high stability
        (1.0 - (std_dev / mean.max(1.0)).min(1.0)).max(0.0)
    }
}

impl DspStage for FingerprintDetector {
    fn name(&self) -> &'static str {
        "S0:Fingerprint"
    }

    fn init(&mut self, max_frame_size: usize, _sample_rate: u32) {
        self.magnitude_buf = vec![0.0; max_frame_size];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut StageContext) {
        self.frame_counter += 1;

        // Only analyze periodically
        if self.frame_counter % self.update_interval != 0 {
            return;
        }

        let cutoff = cutoff::detect_cutoff(samples, ctx.sample_rate, ctx.fft_size);
        let clipping = clipping::detect_clipping(samples);
        let compression = compression::estimate_compression(samples);
        // Stereo analysis: treat interleaved L/R
        let stereo_deg = if ctx.channels >= 2 && samples.len() >= 2 {
            stereo::analyze_stereo_interleaved(samples)
        } else {
            0.0
        };

        // Update cutoff history
        let cutoff_val = cutoff.unwrap_or(20000.0);
        self.cutoff_history[self.cutoff_history_idx % 8] = cutoff_val;
        self.cutoff_history_idx += 1;

        let stability = self.cutoff_stability();
        let quality_tier = Self::classify_quality(cutoff);

        // Spectral flatness as music-likeness indicator
        let is_music_like = true; // simplified for now
        let base_confidence = if is_music_like { 0.8 } else { 0.3 };
        let confidence = (base_confidence * stability).clamp(0.0, 1.0);

        ctx.degradation = DegradationProfile {
            cutoff_freq: cutoff,
            quality_tier,
            clipping_severity: clipping,
            compression_amount: compression,
            stereo_degradation: stereo_deg,
            confidence,
        };
    }

    fn reset(&mut self) {
        self.frame_counter = 0;
        self.cutoff_history = [20000.0; 8];
        self.cutoff_history_idx = 0;
    }
}
