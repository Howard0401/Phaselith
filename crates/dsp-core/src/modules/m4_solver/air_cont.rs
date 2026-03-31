/// Compute air-field continuation residual above cutoff.
///
/// Uses the extrapolated air field from M3 to generate stochastic
/// high-frequency content above the cutoff.
pub fn compute_air_continuation(
    air_field: &[f32],
    cutoff_bin: usize,
    strength: f32,
    residual: &mut [f32],
) {
    let len = residual.len().min(air_field.len());

    for k in cutoff_bin..len {
        residual[k] = air_field[k] * strength;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_continuation_copies_field() {
        let air_field = vec![0.0, 0.0, 0.0, 0.5, 0.3, 0.1];
        let mut residual = vec![0.0; 6];

        compute_air_continuation(&air_field, 3, 1.0, &mut residual);

        assert_eq!(residual[0], 0.0);
        assert_eq!(residual[3], 0.5);
        assert_eq!(residual[4], 0.3);
    }

    #[test]
    fn air_continuation_applies_strength() {
        let air_field = vec![0.0, 0.0, 1.0, 1.0];
        let mut residual = vec![0.0; 4];

        compute_air_continuation(&air_field, 2, 0.5, &mut residual);

        assert!((residual[2] - 0.5).abs() < 1e-6);
    }
}
