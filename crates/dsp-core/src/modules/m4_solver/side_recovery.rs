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
    compute_side_residual_styled(spatial_field, stereo_collapse, strength, 0.3, residual);
}

/// Side recovery with configurable spatial spread.
/// `spatial_spread`: 0.0-1.0, controls ρ_max (0.5-1.0)
pub fn compute_side_residual_styled(
    spatial_field: &[f32],
    stereo_collapse: f32,
    strength: f32,
    spatial_spread: f32,
    residual: &mut [f32],
) {
    let len = residual.len().min(spatial_field.len());
    // spatial_spread 0.0 → ρ_max 0.5 (conservative), 1.0 → ρ_max 1.0 (aggressive)
    let rho_max = 0.5 + spatial_spread * 0.5;

    for k in 0..len {
        let q = spatial_field[k];
        // If Q_spatial is low (collapsed stereo), generate recovery residual
        let deficit = (1.0 - q).max(0.0);
        let recovery = deficit * stereo_collapse * strength;
        residual[k] = recovery.min(rho_max);
    }
}

/// Side recovery with cross-channel stereo bias (gate/bias model).
///
/// Uses real cross-channel context to adjust the aggressiveness of
/// per-bin side recovery. Does NOT replace per-bin spatial_field analysis.
///
/// - High correlation (mono-like) → increase stereo_collapse estimate → more recovery
/// - Low correlation (true stereo) → decrease stereo_collapse estimate → conservative
pub fn compute_side_residual_stereo_biased(
    spatial_field: &[f32],
    cross_channel: &crate::types::CrossChannelContext,
    stereo_collapse: f32,
    strength: f32,
    spatial_spread: f32,
    residual: &mut [f32],
) {
    let len = residual.len().min(spatial_field.len());
    let rho_max = 0.5 + spatial_spread * 0.5;

    // Bias stereo_collapse based on real cross-channel correlation.
    // correlation near 1.0 (mono-like) → boost collapse estimate (more recovery needed)
    // correlation near 0.0 or negative (true stereo) → reduce collapse estimate
    let correlation_bias = cross_channel.correlation.clamp(0.0, 1.0);
    // Blend: original collapse × (0.5 + 0.5 × correlation)
    // At correlation=1: collapse × 1.0 (full aggressiveness)
    // At correlation=0: collapse × 0.5 (half aggressiveness — conservative)
    let biased_collapse = stereo_collapse * (0.5 + 0.5 * correlation_bias);

    for k in 0..len {
        let q = spatial_field[k];
        let deficit = (1.0 - q).max(0.0);
        let recovery = deficit * biased_collapse * strength;
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

    #[test]
    fn stereo_biased_more_aggressive_for_mono() {
        use crate::types::CrossChannelContext;

        let spatial_field = vec![0.2; 100];
        let mut residual_mono = vec![0.0; 100];
        let mut residual_stereo = vec![0.0; 100];

        let mono_ctx = CrossChannelContext {
            correlation: 0.95,
            mid_energy: 1.0,
            side_energy: 0.01,
            stereo_width: 0.01,
        };
        let stereo_ctx = CrossChannelContext {
            correlation: 0.1,
            mid_energy: 0.5,
            side_energy: 0.5,
            stereo_width: 1.0,
        };

        compute_side_residual_stereo_biased(
            &spatial_field, &mono_ctx, 0.8, 1.0, 0.3, &mut residual_mono,
        );
        compute_side_residual_stereo_biased(
            &spatial_field, &stereo_ctx, 0.8, 1.0, 0.3, &mut residual_stereo,
        );

        let mono_total: f32 = residual_mono.iter().sum();
        let stereo_total: f32 = residual_stereo.iter().sum();
        assert!(
            mono_total > stereo_total,
            "Mono-like signal should get more recovery: mono={}, stereo={}",
            mono_total, stereo_total
        );
    }

    #[test]
    fn cross_channel_from_mono_signal() {
        use crate::types::CrossChannelContext;

        let signal: Vec<f32> = (0..128)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let ctx = CrossChannelContext::from_lr(&signal, &signal);
        assert!(ctx.correlation > 0.99, "Mono signal should have correlation ~1, got {}", ctx.correlation);
        assert!(ctx.stereo_width < 0.001, "Mono signal should have near-zero width");
    }

    #[test]
    fn cross_channel_from_stereo_signal() {
        use crate::types::CrossChannelContext;

        let left: Vec<f32> = (0..128)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let right: Vec<f32> = (0..128)
            .map(|i| (2.0 * std::f32::consts::PI * 660.0 * i as f32 / 48000.0).sin())
            .collect();
        let ctx = CrossChannelContext::from_lr(&left, &right);
        assert!(ctx.correlation < 0.9, "Different signals should have low correlation, got {}", ctx.correlation);
    }
}
