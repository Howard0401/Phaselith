/// Apply true peak guard to prevent inter-sample peaks from exceeding limit.
///
/// Uses uniform gain reduction across the entire block instead of per-sample
/// hard clipping. This preserves waveform shape (especially low-frequency
/// content) while still preventing clipping.
pub fn apply_true_peak_guard(samples: &mut [f32], limit: f32) {
    // Find peak amplitude in this block
    let mut peak = 0.0f32;
    for &s in samples.iter() {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
    }

    // If peak exceeds limit, apply uniform gain reduction to entire block
    if peak > limit {
        let gain = limit / peak;
        for s in samples.iter_mut() {
            *s *= gain;
        }
    }
}

/// Per-sample soft saturation clamp.
/// Linear below knee (85% of limit), tanh curve above.
#[inline]
pub fn soft_clamp(x: f32, limit: f32) -> f32 {
    let knee = limit * 0.85;
    let abs_x = x.abs();
    if abs_x <= knee {
        x
    } else {
        let excess = (abs_x - knee) / (limit - knee);
        let compressed = knee + (limit - knee) * excess.tanh();
        x.signum() * compressed
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
