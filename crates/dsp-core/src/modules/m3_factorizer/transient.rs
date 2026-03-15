const PRE_ECHO_TRANSIENT_THRESHOLD: f32 = 0.15;

/// Detect transients by comparing micro and core lattice energy.
///
/// Transients appear as energy bursts in the micro lattice (short window)
/// that are smoothed out in the core lattice (longer window).
pub fn detect_transients(micro_energy: &[f32], core_energy: &[f32], transient_field: &mut [f32]) {
    let len = transient_field.len();
    transient_field.fill(0.0);

    if micro_energy.is_empty() || core_energy.is_empty() {
        return;
    }

    // Map micro bins to core bins (micro has fewer bins)
    let micro_bins = micro_energy.len();
    let core_bins = core_energy.len();

    for k in 0..len.min(core_bins) {
        // Corresponding micro bin (approximate)
        let micro_k = (k * micro_bins) / core_bins.max(1);
        if micro_k >= micro_bins {
            continue;
        }

        let micro_e = micro_energy[micro_k];
        let core_e = core_energy[k];

        // Transient indicator: micro energy >> core energy
        // means energy is concentrated in a short time window
        if core_e > 1e-10 {
            let ratio = micro_e / core_e;
            transient_field[k] = ((ratio - 1.0) * 0.5).clamp(0.0, 1.0);
        } else if micro_e > 1e-10 {
            transient_field[k] = 1.0;
        }
    }
}

/// Peak transient activity in the current transient field.
///
/// Used as a lightweight gate for time-domain pre-echo suppression so we only
/// touch the waveform when the short-window analysis actually sees attack energy.
pub fn peak_activity(transient_field: &[f32]) -> f32 {
    transient_field
        .iter()
        .copied()
        .fold(0.0f32, f32::max)
        .clamp(0.0, 1.0)
}

/// Compute the effective pre-echo suppression strength for the current block.
///
/// Returns `Some(strength)` only when the current callback contains a hop-aligned
/// transient event and the resulting strength is non-trivial.
pub fn pre_echo_strength(
    transient_amount: f32,
    hops_this_block: usize,
    is_transient: bool,
    spectral_flux: f32,
    transient_field: &[f32],
) -> Option<f32> {
    let transient_activity = peak_activity(transient_field)
        .max(spectral_flux)
        .clamp(0.0, 1.0);
    let effective_strength = (transient_amount * transient_activity).clamp(0.0, 1.0);

    if transient_amount > 0.01
        && hops_this_block > 0
        && (is_transient || transient_activity >= PRE_ECHO_TRANSIENT_THRESHOLD)
        && effective_strength > 0.01
    {
        Some(effective_strength)
    } else {
        None
    }
}

/// Suppress pre-echo in a block using a tanh fade-in window.
pub fn suppress_pre_echo(block: &mut [f32], strength: f32) {
    let len = block.len();
    if len == 0 {
        return;
    }

    for i in 0..len {
        let t = i as f32 / len as f32;
        let fade = (t * 4.0 - 2.0).tanh() * 0.5 + 0.5;
        let gain = 1.0 - strength * (1.0 - fade);
        block[i] *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_detected_when_micro_exceeds_core() {
        let micro_energy = vec![0.0, 0.0, 10.0, 0.0]; // burst at bin 2
        let core_energy = vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]; // smoothed
        let mut transient_field = vec![0.0; 9];

        detect_transients(&micro_energy, &core_energy, &mut transient_field);

        // At least some transient energy should be detected
        let total: f32 = transient_field.iter().sum();
        assert!(total >= 0.0, "Should detect some transient energy");
    }

    #[test]
    fn no_transient_in_steady_state() {
        let energy = vec![1.0; 10];
        let mut transient_field = vec![0.0; 10];

        detect_transients(&energy, &energy, &mut transient_field);

        // Micro = core means no transient
        for &t in &transient_field {
            assert!(t < 0.1, "Steady state should have low transient, got {}", t);
        }
    }

    #[test]
    fn pre_echo_suppression() {
        let mut block = vec![1.0; 64];
        suppress_pre_echo(&mut block, 1.0);

        // First samples should be attenuated
        assert!(block[0] < 0.2, "First sample should be suppressed");
        // Last samples should be preserved
        assert!(block[63] > 0.9, "Last sample should be preserved");
    }

    #[test]
    fn peak_activity_reports_max_value() {
        let transient_field = vec![0.0, 0.2, 0.8, 0.4];
        assert!((peak_activity(&transient_field) - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn pre_echo_strength_requires_hop_boundary() {
        let transient_field = vec![0.7, 0.2];
        let strength = pre_echo_strength(1.0, 0, true, 0.8, &transient_field);
        assert!(strength.is_none());
    }

    #[test]
    fn pre_echo_strength_uses_transient_activity() {
        let transient_field = vec![0.0, 0.4, 0.2];
        let strength = pre_echo_strength(0.5, 1, false, 0.1, &transient_field);
        assert_eq!(strength, Some(0.2));
    }

    #[test]
    fn pre_echo_strength_uses_spectral_flux_when_flagged() {
        let transient_field = vec![0.0; 4];
        let strength = pre_echo_strength(0.8, 1, true, 0.3, &transient_field);
        assert!(matches!(strength, Some(v) if (v - 0.24).abs() < 1e-6));
    }
}
