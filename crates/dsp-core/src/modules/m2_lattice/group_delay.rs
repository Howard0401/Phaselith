use crate::types::Lattice;
use super::energy::unwrap_phase;

/// Compute local group delay G_l(m,k) for a lattice.
/// Group delay = -d(phase)/d(frequency), estimated from phase differences.
pub fn compute_group_delay(lattice: &mut Lattice) {
    let num_bins = lattice.phase.len();
    if num_bins < 3 {
        return;
    }

    // Central difference for interior bins
    for k in 1..num_bins - 1 {
        let dp = unwrap_phase(lattice.phase[k + 1] - lattice.phase[k - 1]);
        // Group delay in samples = -N/(2π) * dφ/dk
        // where k is the bin index and dφ/dk is the phase slope per bin
        lattice.group_delay[k] = -dp / (2.0 * std::f32::consts::PI) * lattice.fft_size as f32 / 2.0;
    }

    // Boundary bins: forward/backward difference
    if num_bins >= 2 {
        let dp_start = unwrap_phase(lattice.phase[1] - lattice.phase[0]);
        lattice.group_delay[0] =
            -dp_start / (2.0 * std::f32::consts::PI) * lattice.fft_size as f32;

        let dp_end = unwrap_phase(
            lattice.phase[num_bins - 1] - lattice.phase[num_bins - 2],
        );
        lattice.group_delay[num_bins - 1] =
            -dp_end / (2.0 * std::f32::consts::PI) * lattice.fft_size as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::m2_lattice::stft;

    #[test]
    fn group_delay_of_linear_phase() {
        let fft_size = 256;
        let mut lattice = Lattice::new(fft_size);

        // Linear phase = constant group delay
        let delay_samples = 10.0;
        for k in 0..lattice.phase.len() {
            lattice.phase[k] =
                -2.0 * std::f32::consts::PI * k as f32 * delay_samples / fft_size as f32;
        }

        compute_group_delay(&mut lattice);

        // Interior bins should all have approximately the same group delay
        let interior = &lattice.group_delay[5..lattice.group_delay.len() - 5];
        let mean: f32 = interior.iter().sum::<f32>() / interior.len() as f32;
        for &gd in interior {
            assert!(
                (gd - mean).abs() < 2.0,
                "Group delay should be constant for linear phase, got {} vs mean {}",
                gd,
                mean
            );
        }
    }

    #[test]
    fn group_delay_handles_short_lattice() {
        let mut lattice = Lattice::new(4);
        lattice.phase = vec![0.0, 0.1, 0.2];
        compute_group_delay(&mut lattice);
        // Should not panic
        assert_eq!(lattice.group_delay.len(), 3);
    }

    #[test]
    fn group_delay_from_real_signal() {
        let fft_size = 1024;
        let mut lattice = Lattice::new(fft_size);

        // Generate a delayed impulse
        let mut samples = vec![0.0f32; fft_size];
        let delay = 100;
        samples[delay] = 1.0;

        stft::analyze_lattice(&samples, &mut lattice, 48000);
        compute_group_delay(&mut lattice);

        // Group delay should be non-zero
        let total_gd: f32 = lattice.group_delay.iter().map(|g| g.abs()).sum();
        assert!(total_gd > 0.0, "Group delay should be non-zero for delayed impulse");
    }
}
