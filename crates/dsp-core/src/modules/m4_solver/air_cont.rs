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

/// Smooth air continuation across frames to reduce voiced/unvoiced gaps.
///
/// Uses asymmetric smoothing:
/// - rising air content follows relatively quickly
/// - falling air content decays more slowly to bridge short gaps
///
/// `continuity`:
/// - 0.0 = current behavior
/// - 1.0 = strongest hold / continuity
pub fn smooth_air_continuation(
    residual: &mut [f32],
    state: &mut [f32],
    cutoff_bin: usize,
    continuity: f32,
) {
    let len = residual.len().min(state.len());
    let continuity = continuity.clamp(0.0, 1.0);

    if cutoff_bin > 0 {
        state[..cutoff_bin.min(len)].fill(0.0);
    }

    if len <= cutoff_bin {
        return;
    }

    if continuity <= 0.001 {
        state[cutoff_bin..len].copy_from_slice(&residual[cutoff_bin..len]);
        return;
    }

    let rise_memory = continuity * 0.30;
    let fall_memory = continuity * 0.75;

    for k in cutoff_bin..len {
        let prev = state[k];
        let target = residual[k];
        let memory = if target >= prev {
            rise_memory
        } else {
            fall_memory
        };
        let smoothed = target * (1.0 - memory) + prev * memory;
        residual[k] = smoothed;
        state[k] = smoothed;
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

    #[test]
    fn air_continuity_zero_keeps_current_behavior() {
        let mut residual = vec![0.0, 0.0, 0.6, 0.2];
        let mut state = vec![0.0; 4];

        smooth_air_continuation(&mut residual, &mut state, 2, 0.0);

        assert_eq!(residual, vec![0.0, 0.0, 0.6, 0.2]);
        assert_eq!(state, vec![0.0, 0.0, 0.6, 0.2]);
    }

    #[test]
    fn air_continuity_holds_falling_air_energy() {
        let mut residual = vec![0.0, 0.0, 0.0, 0.0];
        let mut state = vec![0.0, 0.0, 1.0, 0.8];

        smooth_air_continuation(&mut residual, &mut state, 2, 1.0);

        assert!(residual[2] > 0.7, "falling air should hold above cutoff");
        assert!(residual[3] > 0.5, "falling air should decay gradually");
        assert_eq!(state[0], 0.0);
        assert_eq!(state[1], 0.0);
    }
}
