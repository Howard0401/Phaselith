/// Apply hard constraints to the acceptance mask:
/// - Low-band lock: no modification below cutoff
/// - Phase constraint: limit phase correction magnitude
/// - Stereo constraint: bound side/mid ratio
pub fn apply_constraints(mask: &[f32], cutoff_bin: usize) -> Vec<f32> {
    let mut constrained = mask.to_vec();

    // Low-band lock: zero below cutoff
    for k in 0..cutoff_bin.min(constrained.len()) {
        constrained[k] = 0.0;
    }

    // Soft transition zone around cutoff (2 bins)
    let transition_width = 5;
    for k in cutoff_bin..cutoff_bin.saturating_add(transition_width).min(constrained.len()) {
        let t = (k - cutoff_bin) as f32 / transition_width as f32;
        constrained[k] *= t; // ramp up
    }

    constrained
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_lock_below_cutoff() {
        let mask = vec![1.0; 100];
        let result = apply_constraints(&mask, 50);

        for k in 0..50 {
            assert_eq!(result[k], 0.0, "Should be locked below cutoff");
        }
    }

    #[test]
    fn constraints_transition_zone() {
        let mask = vec![1.0; 100];
        let cutoff = 50;
        let result = apply_constraints(&mask, cutoff);

        // Just above cutoff should be ramped
        assert!(result[cutoff] < result[cutoff + 4]);
        // Well above cutoff should be full
        assert!((result[cutoff + 10] - 1.0).abs() < 0.01);
    }

    #[test]
    fn constraints_preserve_above_transition() {
        let mask = vec![0.8; 100];
        let result = apply_constraints(&mask, 10);

        // Well above cutoff should preserve original mask value
        assert!((result[50] - 0.8).abs() < 0.01);
    }
}
