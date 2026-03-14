use crate::types::DamagePosterior;

/// Approximate degradation operator D_θ̂.
///
/// Simulates what the lossy codec would do to the enhanced signal:
/// D_θ̂ = D_cutoff ∘ D_limit ∘ D_pre ∘ D_stereo
///
/// Returns the "re-degraded" signal for comparison with the original.
pub fn approximate_degradation(
    original: &[f32],
    residual: &[f32],
    damage: &DamagePosterior,
    cutoff_bin: usize,
) -> Vec<f32> {
    let len = original.len();
    let mut result = vec![0.0f32; len];
    approximate_degradation_into(original, residual, damage, cutoff_bin, &mut result);
    result
}

/// Zero-alloc variant: writes into pre-allocated `out` buffer.
/// `out` must be at least `original.len()` long.
pub fn approximate_degradation_into(
    original: &[f32],
    residual: &[f32],
    damage: &DamagePosterior,
    cutoff_bin: usize,
    out: &mut [f32],
) {
    let len = original.len().min(out.len());

    // Start with original + residual
    for i in 0..len {
        out[i] = original[i];
        if i < residual.len() {
            out[i] += residual[i];
        }
    }
    // Zero remainder
    for i in len..out.len() {
        out[i] = 0.0;
    }

    // Apply approximate cutoff (zero above cutoff in freq domain approximation)
    // Simplified: attenuate high-index samples (this is a time-domain approx)
    if damage.cutoff.mean < 19500.0 && cutoff_bin > 0 {
        apply_approx_lowpass(&mut out[..len], cutoff_bin, len);
    }

    // Apply approximate limiting
    if damage.limiting.mean > 0.1 {
        let threshold = 1.0 - damage.limiting.mean * 0.3;
        for s in &mut out[..len] {
            *s = soft_clip(*s, threshold);
        }
    }

    // Apply approximate clipping
    if damage.clipping.mean > 0.05 {
        let clip_threshold = 0.99;
        for s in &mut out[..len] {
            *s = s.clamp(-clip_threshold, clip_threshold);
        }
    }
}

/// Simplified lowpass: attenuate the "high frequency" portion.
/// In frequency domain this would be a brick wall at cutoff_bin.
/// Time-domain approximation: moving average to smooth.
fn apply_approx_lowpass(samples: &mut [f32], _cutoff_bin: usize, _len: usize) {
    // Simple 3-tap moving average as rough lowpass approximation
    let n = samples.len();
    if n < 3 {
        return;
    }

    let mut prev = samples[0];
    for i in 1..n - 1 {
        let curr = samples[i];
        let next = samples[i + 1];
        samples[i] = prev * 0.25 + curr * 0.5 + next * 0.25;
        prev = curr;
    }
}

/// Soft clipping using tanh.
#[inline]
fn soft_clip(x: f32, threshold: f32) -> f32 {
    if x.abs() < threshold {
        x
    } else {
        let excess = (x.abs() - threshold) / (1.0 - threshold + 0.01);
        let compressed = threshold + (1.0 - threshold) * excess.tanh();
        compressed * x.signum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrader_with_no_damage_returns_near_original() {
        let original = vec![0.5, -0.3, 0.7, -0.1];
        let residual = vec![0.0; 4];
        let damage = DamagePosterior::default(); // lossless

        let result = approximate_degradation(&original, &residual, &damage, 1000);

        // Should be close to original (no damage applied)
        for (a, b) in original.iter().zip(result.iter()) {
            assert!((a - b).abs() < 0.01, "Should match original: {} vs {}", a, b);
        }
    }

    #[test]
    fn degrader_clips_when_clipping_detected() {
        let original = vec![0.5; 4];
        let residual = vec![0.8; 4]; // will push above 1.0
        let mut damage = DamagePosterior::default();
        damage.clipping.mean = 0.5;

        let result = approximate_degradation(&original, &residual, &damage, 1000);

        for &s in &result {
            assert!(s.abs() <= 0.99, "Should be clipped to 0.99, got {}", s);
        }
    }

    #[test]
    fn soft_clip_identity_below_threshold() {
        assert_eq!(soft_clip(0.5, 0.8), 0.5);
        assert_eq!(soft_clip(-0.3, 0.8), -0.3);
    }

    #[test]
    fn soft_clip_compresses_above_threshold() {
        let clipped = soft_clip(1.5, 0.8);
        assert!(clipped < 1.5, "Should compress");
        assert!(clipped > 0.8, "Should be above threshold");
    }
}
