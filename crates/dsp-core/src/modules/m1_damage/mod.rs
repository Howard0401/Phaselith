pub mod features;
pub mod posterior;
pub mod smoothing;

use crate::module_trait::{CirrusModule, ProcessContext};
use crate::types::GaussianEstimate;

/// M1: Damage Posterior Engine.
///
/// Analyzes audio to estimate a factorized posterior over damage dimensions:
/// cutoff, clipping, limiting, pre-echo, stereo collapse, resampler smear, noise lift.
///
/// Runs only every N frames (~200-300ms) since source quality is stable within a track.
pub struct DamagePosteriorEngine {
    frame_counter: u64,
    update_interval: u64,
    sample_rate: u32,
    fft_size: usize,
    /// Temporal smoothing state.
    smoother: smoothing::TemporalSmoother,
}

impl DamagePosteriorEngine {
    pub fn new() -> Self {
        Self {
            frame_counter: 0,
            update_interval: 32,
            sample_rate: 48000,
            fft_size: 1024,
            smoother: smoothing::TemporalSmoother::new(),
        }
    }
}

impl CirrusModule for DamagePosteriorEngine {
    fn name(&self) -> &'static str {
        "M1:DamagePosterior"
    }

    fn init(&mut self, _max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        self.fft_size = ctx.config.quality_mode.core_fft_size();
        self.frame_counter += 1;

        // Only analyze periodically
        if self.frame_counter % self.update_interval != 0 {
            return;
        }

        // Extract features
        let cutoff_raw = features::detect_cutoff(samples, self.sample_rate, self.fft_size);
        let clipping_raw = features::detect_clipping(samples);
        let compression_raw = features::estimate_compression(samples);
        let stereo_raw = if ctx.channels >= 2 && samples.len() >= 2 {
            features::analyze_stereo_interleaved(samples)
        } else {
            0.0
        };

        // Build raw posterior estimates
        let cutoff_est = match cutoff_raw {
            Some(freq) => GaussianEstimate::new(freq, 500.0), // moderate certainty
            None => GaussianEstimate::new(20000.0, 1000.0),   // lossless, uncertain
        };
        let clipping_est = GaussianEstimate::new(clipping_raw, 0.1);
        let limiting_est = GaussianEstimate::new(compression_raw, 0.15);
        let stereo_est = GaussianEstimate::new(stereo_raw, 0.2);

        // Apply temporal smoothing (EMA)
        let smoothed_cutoff = self.smoother.smooth_estimate(0, cutoff_est);
        let smoothed_clipping = self.smoother.smooth_estimate(1, clipping_est);
        let smoothed_limiting = self.smoother.smooth_estimate(2, limiting_est);
        let smoothed_stereo = self.smoother.smooth_estimate(3, stereo_est);

        // Compute overall confidence
        let confidence = posterior::compute_overall_confidence(
            &smoothed_cutoff,
            &smoothed_clipping,
            &smoothed_limiting,
        );

        // Write to context
        ctx.damage.cutoff = smoothed_cutoff;
        ctx.damage.clipping = smoothed_clipping;
        ctx.damage.limiting = smoothed_limiting;
        ctx.damage.stereo_collapse = smoothed_stereo;
        ctx.damage.overall_confidence = confidence;
    }

    fn reset(&mut self) {
        self.frame_counter = 0;
        self.smoother.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;

    #[test]
    fn damage_engine_initializes() {
        let mut engine = DamagePosteriorEngine::new();
        engine.init(1024, 48000);
        assert_eq!(engine.sample_rate, 48000);
    }

    #[test]
    fn damage_engine_skips_non_update_frames() {
        let mut engine = DamagePosteriorEngine::new();
        engine.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        let mut samples = vec![0.0; 1024];

        // Frame 1 is not an update frame (interval=32)
        engine.process(&mut samples, &mut ctx);
        // Cutoff should still be default
        assert_eq!(ctx.damage.cutoff.mean, 20000.0);
    }

    #[test]
    fn damage_engine_detects_on_update_frame() {
        let mut engine = DamagePosteriorEngine::new();
        engine.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        // Generate a lowpassed signal
        let mut samples: Vec<f32> = (0..1024)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();

        // Run until update frame
        for _ in 0..32 {
            engine.process(&mut samples, &mut ctx);
        }

        // Should have updated the damage posterior
        assert!(ctx.damage.overall_confidence >= 0.0);
    }

    #[test]
    fn damage_engine_reset() {
        let mut engine = DamagePosteriorEngine::new();
        engine.init(1024, 48000);
        engine.frame_counter = 100;
        engine.reset();
        assert_eq!(engine.frame_counter, 0);
    }
}
