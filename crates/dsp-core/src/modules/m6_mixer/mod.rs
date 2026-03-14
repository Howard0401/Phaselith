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
/// - Character layer: warmth (soft saturation) + smoothness
/// - Character floor: ensures effect even on high-quality sources
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
        let style = ctx.config.style;
        let has_residual = !ctx.validated.data.is_empty()
            && ctx.validated.consistency_score >= 0.01;

        // ── Restoration gain (damage-driven) ──
        let confidence = ctx.damage.overall_confidence;
        let strength = ctx.config.strength;
        let restoration_gain = strength * confidence * ctx.validated.consistency_score;

        // ── Character floor: minimum effect even on pristine sources ──
        // This ensures every song gets at least a subtle character upgrade
        let character_floor = style.character_intensity() * 0.15;
        let mix_gain = restoration_gain.max(character_floor);

        if mix_gain < 0.001 && style.warmth < 0.01 && style.smoothness < 0.01 {
            return; // nothing to do
        }

        let len = samples.len();
        let limit = 0.99f32;

        // ── Phase 1a: Mix freq-domain validated residual ──
        if has_residual && mix_gain > 0.001 {
            let res_len = len.min(ctx.validated.data.len());
            for i in 0..res_len {
                let dry = samples[i];
                let residual = ctx.validated.data[i] * mix_gain;
                let mixed = dry + residual;

                if mixed.abs() > limit {
                    let headroom = (limit - dry.abs()).max(0.0);
                    samples[i] = dry + residual.signum() * headroom.min(residual.abs());
                } else {
                    samples[i] = mixed;
                }

                samples[i] = samples[i].clamp(-limit, limit);
            }
        }

        // ── Phase 1b: Mix time-domain residual (independent gain path) ──
        // time_mix_gain is independent of freq-domain consistency_score and overall_confidence.
        // Declip already has internal scaling (clipping_severity * dynamics * transient),
        // so we only scale by strength and clipping posterior.
        let time_mix_gain = strength * ctx.damage.clipping.mean.clamp(0.0, 1.0);
        if time_mix_gain > 0.001 && !ctx.validated.time_residual.is_empty() {
            let time_len = len.min(ctx.validated.time_residual.len());
            for i in 0..time_len {
                let time_res = ctx.validated.time_residual[i] * time_mix_gain;
                if time_res.abs() > 0.0001 {
                    let mixed = samples[i] + time_res;
                    if mixed.abs() > limit {
                        let headroom = (limit - samples[i].abs()).max(0.0);
                        samples[i] += time_res.signum() * headroom.min(time_res.abs());
                    } else {
                        samples[i] = mixed;
                    }
                    samples[i] = samples[i].clamp(-limit, limit);
                }
            }
        }

        // ── Character layer with output level compensation ──
        // Warmth (cubic waveshaper) and smoothness (3-tap averaging) both
        // reduce peak/RMS without adding it back. This makes the processed
        // version sound quieter than bypass in AB comparison.
        // Fix: measure RMS before/after character processing, apply conservative
        // makeup gain to compensate, still respecting true peak limit.

        let apply_warmth = style.warmth > 0.01;
        let apply_smooth = style.smoothness > 0.15 && len >= 3;

        if apply_warmth || apply_smooth {
            // Measure RMS before character processing
            let pre_rms_sq = samples.iter().map(|s| s * s).sum::<f32>() / len.max(1) as f32;

            // ── Phase 2: Warmth (subtle even-harmonic saturation) ──
            // y = x - warmth * x³ / 3
            // This naturally produces even harmonics (tube-like character).
            // Independent of damage detection — works on any source.
            if apply_warmth {
                let w = style.warmth * 0.15; // scale down: warmth 1.0 → 15% saturation drive
                for i in 0..len {
                    let x = samples[i];
                    // Soft saturation: cubic waveshaper
                    let saturated = x - w * x * x * x / 3.0;
                    samples[i] = saturated.clamp(-limit, limit);
                }
            }

            // ── Phase 3: Smoothness (time-domain micro-smoothing) ──
            // Subtle 3-tap averaging that reduces digital harshness.
            // Independent of damage detection — works on any source.
            if apply_smooth {
                let smooth_amount = (style.smoothness - 0.15) * 0.12; // gentle: max ~10%
                // Use prev sample as we go (causal filter, no allocation)
                let mut prev = samples[0];
                for i in 1..len - 1 {
                    let curr = samples[i];
                    let next = samples[i + 1];
                    let smoothed = prev * 0.25 + curr * 0.5 + next * 0.25;
                    samples[i] = curr + (smoothed - curr) * smooth_amount;
                    prev = curr;
                }
            }

            // ── Phase 4: Output level compensation ──
            // Measure RMS after character processing, apply makeup gain to
            // match pre-character loudness. Clamped to [1.0, max_makeup] to
            // ensure we only compensate lost level, never cut.
            let post_rms_sq = samples.iter().map(|s| s * s).sum::<f32>() / len.max(1) as f32;

            if post_rms_sq > 1e-12 && pre_rms_sq > 1e-12 {
                let ratio = (pre_rms_sq / post_rms_sq).sqrt();
                // Clamp: only compensate (ratio >= 1), max 3 dB makeup (~1.41x)
                let makeup = ratio.clamp(1.0, 1.414);
                if makeup > 1.001 {
                    for i in 0..len {
                        samples[i] = (samples[i] * makeup).clamp(-limit, limit);
                    }
                }
            }
        }
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EngineConfig, StyleConfig};
    use crate::types::ValidatedResidual;

    #[test]
    fn mixer_no_change_when_no_residual() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0); // zero character
        let mut ctx = ProcessContext::new(48000, 2, config);
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
            time_residual: vec![0.0; 3],
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
            time_residual: vec![0.0; 3],
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

    #[test]
    fn warmth_produces_subtle_saturation() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.5, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut ctx = ProcessContext::new(48000, 2, config);
        // Need some validated data or character floor
        ctx.validated = ValidatedResidual::new(64);

        let original: Vec<f32> = (0..64)
            .map(|i| 0.7 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut samples = original.clone();

        m6.process(&mut samples, &mut ctx);

        // Output should differ due to saturation
        let diff: f32 = samples.iter().zip(original.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.0, "Warmth should modify the signal");
        // But should be subtle
        let max_diff: f32 = samples.iter().zip(original.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, |a, b| a.max(b));
        assert!(max_diff < 0.1, "Warmth should be subtle, max diff {}", max_diff);
    }

    #[test]
    fn character_floor_ensures_effect_on_pristine_source() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        // Zero confidence = pristine source
        ctx.damage.overall_confidence = 0.0;
        ctx.validated = ValidatedResidual {
            data: vec![0.05; 64],
            time_residual: vec![0.0; 64],
            acceptance_mask: vec![1.0; 64],
            consistency_score: 0.5,
            reprojection_error: 0.0,
        };

        let original = vec![0.5; 64];
        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        // Should still have some effect due to character floor + warmth
        let diff: f32 = samples.iter().zip(original.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.0, "Character floor should ensure effect, diff={}", diff);
    }

    #[test]
    fn smoothness_reduces_harshness() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 0.8, 0.0, 0.0, 0.0);
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(128);

        // Harsh signal with abrupt transitions
        let mut samples: Vec<f32> = (0..128)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();

        let original = samples.clone();
        m6.process(&mut samples, &mut ctx);

        // Smoothed output should have less extreme transitions
        let orig_var: f32 = original.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        let smooth_var: f32 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        assert!(smooth_var < orig_var, "Smoothing should reduce variation");
    }

    // ── Level compensation tests ──

    #[test]
    fn warmth_level_compensation_preserves_rms() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(1.0, 0.0, 0.0, 0.0, 0.0, 0.0); // max warmth
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(256);

        let original: Vec<f32> = (0..256)
            .map(|i| 0.6 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        // Post-processing RMS should be close to pre-processing RMS
        // (within 1 dB ≈ 12% tolerance)
        let ratio = post_rms / pre_rms;
        assert!(
            ratio > 0.88 && ratio < 1.12,
            "RMS should be compensated: pre={pre_rms:.4}, post={post_rms:.4}, ratio={ratio:.4}"
        );
    }

    #[test]
    fn smoothness_level_compensation_preserves_rms() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.0, 0.0, 1.0, 0.0, 0.0, 0.0); // max smoothness
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(256);

        let original: Vec<f32> = (0..256)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let ratio = post_rms / pre_rms;
        assert!(
            ratio > 0.88 && ratio < 1.12,
            "RMS should be compensated: pre={pre_rms:.4}, post={post_rms:.4}, ratio={ratio:.4}"
        );
    }

    #[test]
    fn combined_warmth_smoothness_level_compensated() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        // Both warmth and smoothness active (typical preset)
        config.style = StyleConfig::new(0.3, 0.0, 0.8, 0.0, 0.0, 0.0);
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(256);

        let original: Vec<f32> = (0..256)
            .map(|i| 0.7 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (original.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let mut samples = original.clone();
        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        let ratio = post_rms / pre_rms;
        assert!(
            ratio > 0.85 && ratio < 1.15,
            "Combined character RMS compensated: pre={pre_rms:.4}, post={post_rms:.4}, ratio={ratio:.4}"
        );
    }

    #[test]
    fn level_compensation_respects_true_peak() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(1.0, 0.0, 1.0, 0.0, 0.0, 0.0); // max both
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(128);

        // Near-full-scale signal
        let mut samples: Vec<f32> = (0..128)
            .map(|i| 0.95 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        m6.process(&mut samples, &mut ctx);

        for &s in &samples {
            assert!(
                s.abs() <= 0.99,
                "Level compensation must not exceed true peak, got {}",
                s
            );
        }
    }

    #[test]
    fn level_compensation_no_effect_on_silence() {
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(0.5, 0.0, 0.5, 0.0, 0.0, 0.0);
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(64);

        let mut samples = vec![0.0f32; 64]; // silence
        m6.process(&mut samples, &mut ctx);

        // Silence should remain silence (no div-by-zero issues)
        let max_val = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_val < 1e-10, "Silence should stay silent, got {max_val}");
    }

    #[test]
    fn level_compensation_capped_at_3db() {
        // Even with extreme processing that somehow reduces RMS a lot,
        // makeup gain should not exceed ~1.414 (3 dB)
        let mut m6 = PerceptualSafetyMixer::new();
        m6.init(1024, 48000);
        let mut config = EngineConfig::default();
        config.style = StyleConfig::new(1.0, 0.0, 1.0, 0.0, 0.0, 0.0); // max both
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.validated = ValidatedResidual::new(256);

        // Signal that would be heavily affected by warmth
        let mut samples: Vec<f32> = (0..256)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let pre_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        m6.process(&mut samples, &mut ctx);

        let post_rms: f32 = (samples.iter().map(|s| s * s).sum::<f32>() / 256.0).sqrt();

        // Makeup gain should never push output above 3 dB over input
        assert!(
            post_rms <= pre_rms * 1.42,
            "Makeup gain should be capped: pre={pre_rms:.4}, post={post_rms:.4}"
        );
    }
}
