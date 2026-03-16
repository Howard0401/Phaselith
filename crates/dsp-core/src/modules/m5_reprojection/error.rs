/// Compute reprojection error E_rep(m,k).
///
/// E_rep = |D_θ̂(x + r) - x| for each sample/bin.
/// Low error = consistent restoration.
/// High error = residual is inconsistent with the damage model.
pub fn compute_reprojection_error(
    original: &[f32],
    reprojected: &[f32],
    num_bins: usize,
) -> Vec<f32> {
    let len = num_bins.min(original.len()).min(reprojected.len());
    let mut error = vec![0.0f32; num_bins];

    for i in 0..len {
        error[i] = (reprojected[i] - original[i]).abs();
    }

    error
}

/// Zero-alloc variant: writes into pre-allocated `out` buffer.
#[cfg(feature = "native-rt")]
pub fn compute_reprojection_error_into(
    original: &[f32],
    reprojected: &[f32],
    num_bins: usize,
    out: &mut [f32],
) {
    let len = num_bins.min(original.len()).min(reprojected.len()).min(out.len());
    for i in 0..len {
        out[i] = (reprojected[i] - original[i]).abs();
    }
    for i in len..out.len().min(num_bins) {
        out[i] = 0.0;
    }
}

/// Compute total reprojection cost J_rep = Σ E_rep²(m,k) / N.
pub fn total_reprojection_cost(error: &[f32]) -> f32 {
    if error.is_empty() {
        return 0.0;
    }
    error.iter().map(|e| e * e).sum::<f32>() / error.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_error_for_identical() {
        let original = vec![1.0, 2.0, 3.0];
        let reprojected = vec![1.0, 2.0, 3.0];

        let error = compute_reprojection_error(&original, &reprojected, 3);
        let cost = total_reprojection_cost(&error);

        assert_eq!(cost, 0.0);
    }

    #[test]
    fn nonzero_error_for_different() {
        let original = vec![1.0, 2.0, 3.0];
        let reprojected = vec![1.1, 2.2, 3.3];

        let error = compute_reprojection_error(&original, &reprojected, 3);
        let cost = total_reprojection_cost(&error);

        assert!(cost > 0.0);
    }

    #[test]
    fn error_proportional_to_difference() {
        let original = vec![0.0; 10];
        let small_diff: Vec<f32> = vec![0.01; 10];
        let big_diff: Vec<f32> = vec![0.5; 10];

        let small_cost = total_reprojection_cost(
            &compute_reprojection_error(&original, &small_diff, 10),
        );
        let big_cost = total_reprojection_cost(
            &compute_reprojection_error(&original, &big_diff, 10),
        );

        assert!(big_cost > small_cost);
    }
}
