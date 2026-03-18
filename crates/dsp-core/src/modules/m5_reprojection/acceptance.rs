/// Compute acceptance mask A(m,k) from reprojection error.
///
/// A(m,k) = 1 if E_rep < threshold → accept the residual
/// A(m,k) = shrunk value if E_rep >= threshold → partially reject
pub fn compute_acceptance_mask(error: &[f32], cutoff_bin: usize) -> Vec<f32> {
    compute_acceptance_mask_dynamic(error, cutoff_bin, 0.6)
}

/// Acceptance mask with dynamics-controlled threshold.
/// Higher `dynamics` → more lenient threshold → more residual accepted.
///
/// Uses Wiener-inspired soft gain: instead of a hard threshold that creates
/// abrupt accept/reject boundaries, each bin gets a smooth gain based on
/// its signal-to-noise ratio (residual vs reprojection error).
///
/// Wiener gain: G[k] = R²[k] / (R²[k] + E²[k] + ε)
/// - When residual >> error: G → 1 (fully accept)
/// - When error >> residual: G → spectral_floor (almost reject)
///
/// The dynamics parameter controls the spectral floor:
/// - dynamics 0.0 → floor = 0.02 (strict, more nulling)
/// - dynamics 1.0 → floor = 0.05 (lenient, keeps more residual)
pub fn compute_acceptance_mask_dynamic(
    error: &[f32],
    cutoff_bin: usize,
    dynamics: f32,
) -> Vec<f32> {
    compute_wiener_mask(error, &[], cutoff_bin, dynamics)
}

/// Wiener soft mask with explicit residual magnitudes.
/// If `residual` is empty, falls back to threshold-based approach
/// using error statistics (backward compatible).
pub fn compute_wiener_mask(
    error: &[f32],
    residual: &[f32],
    cutoff_bin: usize,
    dynamics: f32,
) -> Vec<f32> {
    let len = error.len();
    let mut mask = vec![1.0f32; len];
    let dynamics_clamped = dynamics.clamp(0.0, 1.0);

    // Spectral floor: prevents complete nulling of any bin.
    // Conservative range: 0.02 (strict) to 0.05 (lenient).
    let spectral_floor = 0.02 + dynamics_clamped * 0.03;

    if !residual.is_empty() {
        // Wiener soft gain: G[k] = R²/(R² + E² + ε)
        let epsilon = 1e-12_f32;
        for k in 0..len {
            let r = if k < residual.len() { residual[k] } else { 0.0 };
            let e = error[k];
            let r_sq = r * r;
            let e_sq = e * e;
            let wiener = r_sq / (r_sq + e_sq + epsilon);
            let wiener = if wiener.is_finite() { wiener } else { spectral_floor };
            mask[k] = wiener.max(spectral_floor);

            // Below cutoff: no residual allowed (low-band lock)
            if k < cutoff_bin {
                mask[k] = 0.0;
            }
        }
    } else {
        // Fallback: threshold-based soft mask (when residual not available).
        // Uses adaptive threshold from error statistics, but applies soft
        // transition instead of hard accept/reject.
        let mad_mult = 2.5 - dynamics_clamped * 1.0;
        let threshold = compute_adaptive_threshold_scaled(error, mad_mult);

        for k in 0..len {
            if error[k] > threshold {
                // Soft shrink: smooth transition around threshold
                let ratio = threshold / error[k].max(1e-10);
                mask[k] = ratio.max(spectral_floor);
            }

            if k < cutoff_bin {
                mask[k] = 0.0;
            }
        }
    }

    mask
}

/// Compute adaptive threshold based on error distribution.
/// Uses median + MAD (median absolute deviation) for robustness.
#[allow(dead_code)]
fn compute_adaptive_threshold(error: &[f32]) -> f32 {
    compute_adaptive_threshold_scaled(error, 2.0)
}

fn compute_adaptive_threshold_scaled(error: &[f32], mad_multiplier: f32) -> f32 {
    if error.is_empty() {
        return 0.01;
    }

    let mut sorted: Vec<f32> = error.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median = sorted[sorted.len() / 2];

    // MAD = median(|x - median|)
    let mut deviations: Vec<f32> = sorted.iter().map(|&x| (x - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = deviations[deviations.len() / 2];

    // Threshold = median + multiplier * MAD
    (median + mad_multiplier * mad).max(0.01)
}

/// Zero-alloc Wiener mask: writes into pre-allocated `out` buffer.
#[cfg(feature = "native-rt")]
pub fn compute_wiener_mask_into(
    error: &[f32],
    residual: &[f32],
    cutoff_bin: usize,
    dynamics: f32,
    out: &mut [f32],
) {
    let len = error.len().min(out.len());
    let dynamics_clamped = dynamics.clamp(0.0, 1.0);
    let spectral_floor = 0.02 + dynamics_clamped * 0.03;

    if !residual.is_empty() {
        let epsilon = 1e-12_f32;
        for k in 0..len {
            let r = if k < residual.len() { residual[k] } else { 0.0 };
            let e = error[k];
            let r_sq = r * r;
            let e_sq = e * e;
            let wiener = r_sq / (r_sq + e_sq + epsilon);
            let wiener = if wiener.is_finite() { wiener } else { spectral_floor };
            out[k] = wiener.max(spectral_floor);
            if k < cutoff_bin {
                out[k] = 0.0;
            }
        }
    } else {
        // For native-rt, use a simplified threshold approach that avoids sorting
        // (sorting requires allocation for the sorted copy)
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for k in 0..len {
            sum += error[k];
            count += 1;
        }
        let mean = if count > 0 { sum / count as f32 } else { 0.01 };
        let threshold = (mean * 2.0).max(0.01);

        for k in 0..len {
            if error[k] > threshold {
                let ratio = threshold / error[k].max(1e-10);
                out[k] = ratio.max(spectral_floor);
            } else {
                out[k] = 1.0;
            }
            if k < cutoff_bin {
                out[k] = 0.0;
            }
        }
    }
}

/// Apply constrained shrinkage to the residual.
/// α(m,k) = min(1, τ / E_rep(m,k))
pub fn shrink_residual(residual: &mut [f32], mask: &[f32]) {
    for k in 0..residual.len().min(mask.len()) {
        residual[k] *= mask[k];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_accepts_low_error() {
        let error = vec![0.001, 0.002, 0.001, 0.003];
        let mask = compute_acceptance_mask(&error, 0);

        for &m in &mask {
            assert!(m > 0.5, "Low error should be mostly accepted, got {}", m);
        }
    }

    #[test]
    fn mask_rejects_high_error() {
        let mut error = vec![0.001; 100];
        error[50] = 10.0; // outlier

        let mask = compute_acceptance_mask(&error, 0);
        assert!(mask[50] < 0.1, "High error should be rejected, got {}", mask[50]);
    }

    #[test]
    fn mask_locks_below_cutoff() {
        let error = vec![0.001; 100];
        let cutoff_bin = 50;

        let mask = compute_acceptance_mask(&error, cutoff_bin);

        for k in 0..cutoff_bin {
            assert_eq!(mask[k], 0.0, "Below cutoff should be locked to 0");
        }
    }

    #[test]
    fn wiener_mask_soft_transition() {
        // Wiener mask should give smooth values between floor and 1.0
        let error = vec![0.01, 0.05, 0.1, 0.5, 1.0];
        let residual = vec![0.1, 0.1, 0.1, 0.1, 0.1];
        let mask = compute_wiener_mask(&error, &residual, 0, 0.5);

        // High residual / low error → high gain
        assert!(mask[0] > 0.9, "Low error should give high gain, got {}", mask[0]);
        // Low residual / high error → near floor
        assert!(mask[4] < 0.1, "High error should give low gain, got {}", mask[4]);
        // All values should be between floor and 1.0
        for &m in &mask {
            assert!(m >= 0.02 && m <= 1.0, "Mask out of range: {m}");
        }
    }

    #[test]
    fn wiener_mask_respects_spectral_floor() {
        // Even with huge error, mask should not go below floor
        let error = vec![100.0; 10];
        let residual = vec![0.001; 10];
        let mask_strict = compute_wiener_mask(&error, &residual, 0, 0.0);
        let mask_lenient = compute_wiener_mask(&error, &residual, 0, 1.0);

        for &m in &mask_strict {
            assert!(m >= 0.019, "Strict floor should be ~0.02, got {m}");
        }
        for &m in &mask_lenient {
            assert!(m >= 0.049, "Lenient floor should be ~0.05, got {m}");
        }
    }

    #[test]
    fn wiener_fallback_without_residual() {
        // When residual is empty, should fall back to threshold-based
        let error = vec![0.001, 0.002, 0.001, 0.003];
        let mask = compute_wiener_mask(&error, &[], 0, 0.6);
        for &m in &mask {
            assert!(m > 0.02, "Fallback should still give valid mask, got {m}");
        }
    }

    #[test]
    fn shrink_residual_applies_mask() {
        let mut residual = vec![1.0; 4];
        let mask = vec![1.0, 0.5, 0.0, 0.8];

        shrink_residual(&mut residual, &mask);

        assert_eq!(residual[0], 1.0);
        assert_eq!(residual[1], 0.5);
        assert_eq!(residual[2], 0.0);
        assert!((residual[3] - 0.8).abs() < 1e-6);
    }
}
