use crate::types::GaussianEstimate;

/// Compute overall detection confidence from individual damage estimates.
/// Combines the confidence of each estimate, weighted by its relevance.
pub fn compute_overall_confidence(
    cutoff: &GaussianEstimate,
    clipping: &GaussianEstimate,
    limiting: &GaussianEstimate,
) -> f32 {
    // Cutoff detection is the most important signal
    let cutoff_conf = cutoff.confidence();
    let clipping_conf = clipping.confidence();
    let limiting_conf = limiting.confidence();

    // Weighted average: cutoff matters most
    let weighted = cutoff_conf * 0.5 + clipping_conf * 0.25 + limiting_conf * 0.25;
    weighted.clamp(0.0, 1.0)
}

/// Combine two independent Gaussian estimates (product of Gaussians).
/// Used for fusing estimates from different time windows.
pub fn fuse_estimates(a: &GaussianEstimate, b: &GaussianEstimate) -> GaussianEstimate {
    if a.variance < 1e-10 {
        return *a;
    }
    if b.variance < 1e-10 {
        return *b;
    }

    let combined_variance = 1.0 / (1.0 / a.variance + 1.0 / b.variance);
    let combined_mean =
        combined_variance * (a.mean / a.variance + b.mean / b.variance);

    GaussianEstimate::new(combined_mean, combined_variance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_confidence_high_precision() {
        let cutoff = GaussianEstimate::new(15000.0, 0.01);
        let clipping = GaussianEstimate::new(0.5, 0.01);
        let limiting = GaussianEstimate::new(0.3, 0.01);

        let conf = compute_overall_confidence(&cutoff, &clipping, &limiting);
        assert!(conf > 0.8, "High precision should give high confidence, got {}", conf);
    }

    #[test]
    fn overall_confidence_high_uncertainty() {
        let cutoff = GaussianEstimate::new(15000.0, 1000.0);
        let clipping = GaussianEstimate::new(0.5, 100.0);
        let limiting = GaussianEstimate::new(0.3, 100.0);

        let conf = compute_overall_confidence(&cutoff, &clipping, &limiting);
        assert!(conf < 0.3, "High uncertainty should give low confidence, got {}", conf);
    }

    #[test]
    fn fuse_estimates_combines_correctly() {
        let a = GaussianEstimate::new(10.0, 1.0);
        let b = GaussianEstimate::new(12.0, 1.0);

        let fused = fuse_estimates(&a, &b);
        // Mean should be between 10 and 12
        assert!(fused.mean > 10.0 && fused.mean < 12.0);
        // Variance should be less than either input
        assert!(fused.variance < 1.0);
    }

    #[test]
    fn fuse_estimates_precise_dominates() {
        let precise = GaussianEstimate::new(15000.0, 0.01);
        let uncertain = GaussianEstimate::new(10000.0, 1000.0);

        let fused = fuse_estimates(&precise, &uncertain);
        // Should be close to the precise estimate
        assert!((fused.mean - 15000.0).abs() < 500.0);
    }
}
