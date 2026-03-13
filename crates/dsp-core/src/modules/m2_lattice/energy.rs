use crate::types::Lattice;

/// Compute energy field P_l(m,k) = |X(m,k)|² for a lattice.
/// This is already done in stft::analyze_lattice, but this function
/// can recompute from magnitude if needed.
pub fn compute_energy_field(lattice: &mut Lattice) {
    for i in 0..lattice.magnitude.len() {
        lattice.energy[i] = lattice.magnitude[i] * lattice.magnitude[i];
    }
}

/// Compute phase stability between current and previous phase arrays.
/// Returns per-bin stability (0=unstable, 1=stable).
pub fn phase_stability(current_phase: &[f32], prev_phase: &[f32], hop_size: usize) -> Vec<f32> {
    let len = current_phase.len().min(prev_phase.len());
    let mut stability = vec![0.0f32; len];

    for k in 0..len {
        // Expected phase advance for bin k over one hop
        let expected_advance =
            2.0 * std::f32::consts::PI * k as f32 * hop_size as f32
                / (len as f32 * 2.0); // fft_size ≈ len*2

        let actual_advance = unwrap_phase(current_phase[k] - prev_phase[k]);
        let deviation = unwrap_phase(actual_advance - expected_advance).abs();

        // Map deviation to stability: 0 deviation = 1.0 stability
        stability[k] = (-deviation * deviation / 0.5).exp();
    }

    stability
}

/// Unwrap phase to [-π, π].
#[inline]
pub fn unwrap_phase(phase: f32) -> f32 {
    let mut p = phase;
    while p > std::f32::consts::PI {
        p -= 2.0 * std::f32::consts::PI;
    }
    while p < -std::f32::consts::PI {
        p += 2.0 * std::f32::consts::PI;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_field_from_magnitude() {
        let mut lattice = Lattice::new(256);
        lattice.magnitude[10] = 0.5;
        lattice.magnitude[20] = 1.0;

        compute_energy_field(&mut lattice);

        assert!((lattice.energy[10] - 0.25).abs() < 1e-6);
        assert!((lattice.energy[20] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn phase_stability_stable_signal() {
        let hop_size = 256;
        let num_bins = 513;

        // Generate phase arrays where phase advances at the expected rate
        let prev_phase: Vec<f32> = (0..num_bins)
            .map(|k| k as f32 * 0.1)
            .collect();
        let current_phase: Vec<f32> = (0..num_bins)
            .map(|k| {
                let expected =
                    2.0 * std::f32::consts::PI * k as f32 * hop_size as f32
                        / (num_bins as f32 * 2.0);
                k as f32 * 0.1 + expected
            })
            .collect();

        let stab = phase_stability(&current_phase, &prev_phase, hop_size);
        let avg_stab: f32 = stab.iter().sum::<f32>() / stab.len() as f32;
        assert!(avg_stab > 0.5, "Stable signal should have high stability, got {}", avg_stab);
    }

    #[test]
    fn unwrap_phase_wraps() {
        assert!((unwrap_phase(4.0) - (4.0 - 2.0 * std::f32::consts::PI)).abs() < 1e-5);
        assert!((unwrap_phase(-4.0) - (-4.0 + 2.0 * std::f32::consts::PI)).abs() < 1e-5);
    }
}
