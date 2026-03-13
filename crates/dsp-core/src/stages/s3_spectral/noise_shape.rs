use rustfft::num_complex::Complex;

/// Add shaped noise above the cutoff to restore "air" and breathiness.
///
/// Returns the updated noise phase state.
pub fn add_shaped_noise(
    spectrum: &mut [Complex<f32>],
    noise_envelope: &[f32],
    cutoff_bin: usize,
    bin_to_freq: f32,
    strength: f32,
    mut noise_phase: f32,
) -> f32 {
    if strength < 0.001 {
        return noise_phase;
    }

    for bin in cutoff_bin..spectrum.len() {
        let freq = bin as f32 * bin_to_freq;
        if freq > 22000.0 {
            break;
        }

        // Extrapolate noise level from the envelope near cutoff
        let noise_level = if bin < noise_envelope.len() && noise_envelope[bin] > 0.0 {
            noise_envelope[bin]
        } else if cutoff_bin > 0 && cutoff_bin < noise_envelope.len() {
            // Extrapolate with decay
            let distance = (bin - cutoff_bin) as f32;
            noise_envelope[cutoff_bin.min(noise_envelope.len() - 1)]
                * (-distance * 0.05).exp()
        } else {
            0.0
        };

        // Deterministic "noise" phase (reproducible)
        noise_phase = (noise_phase + 2.71828 * bin as f32) % (2.0 * std::f32::consts::PI);

        let noise_mag = noise_level * strength;
        spectrum[bin] += Complex::from_polar(noise_mag, noise_phase);
    }

    noise_phase
}
