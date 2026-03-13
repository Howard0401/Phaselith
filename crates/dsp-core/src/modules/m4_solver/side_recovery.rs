/// Compute bounded stereo side recovery residual.
///
/// Attempts to restore stereo width while respecting ρ_max constraint
/// (maximum side/mid ratio).
pub fn compute_side_residual(
    spatial_field: &[f32],
    stereo_collapse: f32,
    strength: f32,
    residual: &mut [f32],
) {
    let len = residual.len().min(spatial_field.len());
    let rho_max = 0.8; // maximum side/mid ratio limit

    for k in 0..len {
        let q = spatial_field[k];
        // If Q_spatial is low (collapsed stereo), generate recovery residual
        let deficit = (1.0 - q).max(0.0);
        let recovery = deficit * stereo_collapse * strength;
        residual[k] = recovery.min(rho_max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_recovery_for_collapsed_stereo() {
        let spatial_field = vec![0.1; 100]; // low Q = collapsed
        let mut residual = vec![0.0; 100];

        compute_side_residual(&spatial_field, 0.8, 1.0, &mut residual);

        assert!(residual[50] > 0.0, "Should have recovery residual");
        assert!(residual[50] <= 0.8, "Should not exceed rho_max");
    }

    #[test]
    fn side_recovery_zero_for_good_stereo() {
        let spatial_field = vec![1.0; 100]; // perfect stereo
        let mut residual = vec![0.0; 100];

        compute_side_residual(&spatial_field, 0.0, 1.0, &mut residual);

        let total: f32 = residual.iter().sum();
        assert_eq!(total, 0.0, "No recovery needed for good stereo");
    }

    #[test]
    fn side_recovery_respects_rho_max() {
        let spatial_field = vec![0.0; 100]; // fully collapsed
        let mut residual = vec![0.0; 100];

        compute_side_residual(&spatial_field, 1.0, 1.0, &mut residual);

        for &r in &residual {
            assert!(r <= 0.8, "Should not exceed rho_max, got {}", r);
        }
    }
}
