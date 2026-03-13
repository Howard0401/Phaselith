use crate::modules::m2_lattice::energy::unwrap_phase;

/// Compute phase-field relaxation residual.
///
/// Ensures phase coherence above cutoff by extrapolating group delay
/// from below the cutoff. The residual represents the phase correction needed.
pub fn compute_phase_residual(
    phase: &[f32],
    cutoff_bin: usize,
    residual: &mut [f32],
) {
    let num_bins = phase.len().min(residual.len());
    if cutoff_bin < 3 || cutoff_bin >= num_bins {
        return;
    }

    // Compute phase slope near cutoff (group delay approximation)
    let p1 = phase[cutoff_bin.saturating_sub(2)];
    let p2 = phase[cutoff_bin.saturating_sub(1)];
    let phase_slope = unwrap_phase(p2 - p1);
    let ref_phase = p2;

    // Above cutoff: compute expected phase and store deviation as residual
    for k in cutoff_bin..num_bins {
        let expected = ref_phase + phase_slope * (k - cutoff_bin + 1) as f32;
        let current = phase[k];
        let deviation = unwrap_phase(expected - current);

        // Only correct significant deviations
        if deviation.abs() > 0.1 {
            residual[k] = deviation * 0.5; // gentle correction
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_residual_for_random_phase() {
        let num_bins = 100;
        let cutoff_bin = 50;

        // Linear phase below cutoff, random above
        let mut phase = vec![0.0; num_bins];
        let slope = 0.1;
        for k in 0..cutoff_bin {
            phase[k] = k as f32 * slope;
        }
        for k in cutoff_bin..num_bins {
            phase[k] = k as f32 * 0.7; // wrong phase
        }

        let mut residual = vec![0.0; num_bins];
        compute_phase_residual(&phase, cutoff_bin, &mut residual);

        // Should have non-zero residual above cutoff
        let above_total: f32 = residual[cutoff_bin..].iter().map(|r| r.abs()).sum();
        assert!(above_total > 0.0, "Should correct phase above cutoff");
    }

    #[test]
    fn phase_residual_zero_for_coherent() {
        let num_bins = 100;
        let cutoff_bin = 50;

        // Linear phase everywhere
        let slope = 0.1;
        let phase: Vec<f32> = (0..num_bins).map(|k| k as f32 * slope).collect();

        let mut residual = vec![0.0; num_bins];
        compute_phase_residual(&phase, cutoff_bin, &mut residual);

        let above_total: f32 = residual[cutoff_bin..].iter().map(|r| r.abs()).sum();
        assert!(
            above_total < 1.0,
            "Coherent phase should have minimal residual, got {}",
            above_total
        );
    }

    #[test]
    fn phase_residual_below_cutoff_untouched() {
        let phase = vec![0.0; 100];
        let mut residual = vec![0.0; 100];
        compute_phase_residual(&phase, 50, &mut residual);

        let below: f32 = residual[..50].iter().map(|r| r.abs()).sum();
        assert_eq!(below, 0.0, "Below cutoff should be untouched");
    }
}
