/// Compute acceptance mask A(m,k) from reprojection error.
///
/// A(m,k) = 1 if E_rep < threshold → accept the residual
/// A(m,k) = shrunk value if E_rep >= threshold → partially reject
pub fn compute_acceptance_mask(error: &[f32], cutoff_bin: usize) -> Vec<f32> {
    let len = error.len();
    let mut mask = vec![1.0f32; len];

    // Compute threshold from error statistics
    let threshold = compute_adaptive_threshold(error);

    for k in 0..len {
        if error[k] > threshold {
            // Shrink: α(m,k) = threshold / E_rep(m,k)
            mask[k] = (threshold / error[k].max(1e-10)).clamp(0.0, 1.0);
        }

        // Below cutoff: always accept (low-band lock)
        if k < cutoff_bin {
            mask[k] = 0.0; // no residual allowed below cutoff
        }
    }

    mask
}

/// Compute adaptive threshold based on error distribution.
/// Uses median + MAD (median absolute deviation) for robustness.
fn compute_adaptive_threshold(error: &[f32]) -> f32 {
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

    // Threshold = median + 2 * MAD (reject outliers)
    (median + 2.0 * mad).max(0.01)
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
