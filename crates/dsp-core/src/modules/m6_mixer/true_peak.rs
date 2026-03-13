/// Apply true peak guard to prevent inter-sample peaks from exceeding limit.
///
/// Simple implementation: hard limits samples and applies a tiny fade
/// to avoid discontinuities.
pub fn apply_true_peak_guard(samples: &mut [f32], limit: f32) {
    for i in 0..samples.len() {
        if samples[i].abs() > limit {
            samples[i] = samples[i].signum() * limit;

            // Smooth the transition to avoid a discontinuity
            if i > 0 {
                let blend = 0.5;
                samples[i] = samples[i] * blend + samples[i - 1] * (1.0 - blend);
                samples[i] = samples[i].clamp(-limit, limit);
            }
        }
    }
}

/// Check if any sample exceeds the true peak limit.
pub fn exceeds_true_peak(samples: &[f32], limit: f32) -> bool {
    samples.iter().any(|s| s.abs() > limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_peak_clamps() {
        let mut samples = vec![0.5, 1.5, -2.0, 0.3];
        apply_true_peak_guard(&mut samples, 0.99);

        for &s in &samples {
            assert!(s.abs() <= 0.99, "Should not exceed limit, got {}", s);
        }
    }

    #[test]
    fn true_peak_preserves_quiet() {
        let mut samples = vec![0.1, -0.2, 0.3, -0.4];
        let original = samples.clone();
        apply_true_peak_guard(&mut samples, 0.99);
        assert_eq!(samples, original);
    }

    #[test]
    fn exceeds_detection() {
        assert!(exceeds_true_peak(&[0.5, 1.5, 0.3], 0.99));
        assert!(!exceeds_true_peak(&[0.5, 0.8, 0.3], 0.99));
    }
}
