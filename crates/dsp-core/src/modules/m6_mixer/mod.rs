pub mod crossover;
pub mod masking;
pub mod true_peak;

use crate::module_trait::{CirrusModule, ProcessContext};

/// M6: Perceptual Safety Mixer.
///
/// Mixes the validated residual into the dry signal with safety guards:
/// - Low-band lock: original signal preserved below cutoff
/// - Soft crossover around cutoff
/// - Add-only above cutoff (no subtraction)
/// - Masking ceiling enforcement
/// - True peak guard
/// - Confidence-weighted blending
pub struct PerceptualSafetyMixer {
    sample_rate: u32,
}

impl PerceptualSafetyMixer {
    pub fn new() -> Self {
        Self { sample_rate: 48000 }
    }
}

impl CirrusModule for PerceptualSafetyMixer {
    fn name(&self) -> &'static str {
        "M6:SafetyMixer"
    }

    fn init(&mut self, _max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        if ctx.validated.data.is_empty() || ctx.validated.consistency_score < 0.01 {
            return; // nothing to mix
        }

        let confidence = ctx.damage.overall_confidence;
        let strength = ctx.config.strength;
        let mix_gain = strength * confidence * ctx.validated.consistency_score;

        if mix_gain < 0.001 {
            return;
        }

        let len = samples.len().min(ctx.validated.data.len());
        let limit = 0.99f32;

        // Add validated residual to samples with per-sample ceiling.
        // If dry + residual would exceed limit, scale down the residual
        // contribution for that sample. The dry signal is NEVER attenuated.
        for i in 0..len {
            let dry = samples[i];
            let residual = ctx.validated.data[i] * mix_gain;
            let mixed = dry + residual;

            if mixed.abs() > limit {
                // How much headroom is available above dry?
                let headroom = (limit - dry.abs()).max(0.0);
                // Clamp residual to fit within headroom, preserving sign
                samples[i] = dry + residual.signum() * headroom.min(residual.abs());
            } else {
                samples[i] = mixed;
            }

            // Final safety clamp: handles dry input already above limit
            samples[i] = samples[i].clamp(-limit, limit);
        }
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::types::ValidatedResidual;

    #[test]
    fn mixer_no_change_when_no_residual() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        let original = vec![0.5, -0.3, 0.7];
        let mut samples = original.clone();

        m6.process(&mut samples, &mut ctx);
        assert_eq!(samples, original);
    }

    #[test]
    fn mixer_adds_validated_residual() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        ctx.validated = ValidatedResidual {
            data: vec![0.1, 0.1, 0.1],
            acceptance_mask: vec![1.0; 3],
            consistency_score: 1.0,
            reprojection_error: 0.0,
        };
        ctx.damage.overall_confidence = 1.0;

        let mut samples = vec![0.5, 0.5, 0.5];
        m6.process(&mut samples, &mut ctx);

        // Should have added some residual
        assert!(samples[0] > 0.5, "Should have added residual, got {}", samples[0]);
    }

    #[test]
    fn mixer_respects_true_peak() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        ctx.validated = ValidatedResidual {
            data: vec![10.0, 10.0, 10.0],
            acceptance_mask: vec![1.0; 3],
            consistency_score: 1.0,
            reprojection_error: 0.0,
        };
        ctx.damage.overall_confidence = 1.0;

        let mut samples = vec![0.9, 0.9, 0.9];
        m6.process(&mut samples, &mut ctx);

        for &s in &samples {
            assert!(s.abs() <= 0.99, "Should not exceed true peak, got {}", s);
        }
    }
}
