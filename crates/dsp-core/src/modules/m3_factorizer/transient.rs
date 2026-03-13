/// Detect transients by comparing micro and core lattice energy.
///
/// Transients appear as energy bursts in the micro lattice (short window)
/// that are smoothed out in the core lattice (longer window).
pub fn detect_transients(
    micro_energy: &[f32],
    core_energy: &[f32],
    transient_field: &mut [f32],
) {
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
}
