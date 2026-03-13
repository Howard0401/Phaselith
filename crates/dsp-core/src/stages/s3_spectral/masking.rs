use rustfft::num_complex::Complex;

/// Apply psychoacoustic masking constraint to prevent over-reconstruction.
///
/// Reconstructed content that exceeds the masking threshold will sound
/// "fake" or "harsh". This limits the reconstruction to stay below
/// the perceptual masking curve.
pub fn apply_masking_constraint(
    spectrum: &mut [Complex<f32>],
    cutoff_bin: usize,
    bin_to_freq: f32,
) {
    for bin in cutoff_bin..spectrum.len() {
        let freq = bin as f32 * bin_to_freq;
        if freq > 22000.0 {
            break;
        }

        let threshold = masking_threshold(freq, spectrum, bin_to_freq);
        let current = spectrum[bin].norm();

        if current > threshold * 1.5 {
            spectrum[bin] *= threshold * 1.5 / current;
        }
    }
}

/// Simplified masking threshold calculation.
///
/// Combines absolute hearing threshold (Terhardt approximation)
/// with simultaneous masking from nearby spectral energy.
fn masking_threshold(freq: f32, spectrum: &[Complex<f32>], bin_to_freq: f32) -> f32 {
    let absolute = absolute_hearing_threshold(freq);

    // Simultaneous masking: energy from nearby bands
    let bark = hz_to_bark(freq);
    let mut masking = 0.0f32;

    // Sample a few nearby bands for masking spread
    for offset in [-3.0f32, -2.0, -1.0, 1.0, 2.0, 3.0] {
        let neighbor_bark = bark + offset;
        let neighbor_freq = bark_to_hz(neighbor_bark);
        let neighbor_bin = (neighbor_freq / bin_to_freq) as usize;

        if neighbor_bin < spectrum.len() {
            let energy = spectrum[neighbor_bin].norm_sqr();
            let distance = offset.abs();
            // Asymmetric masking spread
            let spread = if offset > 0.0 {
                (-3.0 * distance).exp() * energy // Low→high: wider
            } else {
                (-5.0 * distance).exp() * energy // High→low: narrower
            };
            masking = masking.max(spread.sqrt());
        }
    }

    let mask_offset = 0.5; // -6dB masking offset
    absolute.max(masking * mask_offset)
}

/// Absolute hearing threshold (simplified Terhardt approximation).
fn absolute_hearing_threshold(freq: f32) -> f32 {
    let f_khz = freq / 1000.0;
    let threshold_db = 3.64 * f_khz.powf(-0.8)
        - 6.5 * (-0.6 * (f_khz - 3.3).powi(2)).exp()
        + 1e-3 * f_khz.powi(4);
    db_to_linear(threshold_db - 96.0)
}

fn hz_to_bark(freq: f32) -> f32 {
    13.0 * (0.00076 * freq).atan() + 3.5 * (freq / 7500.0).powi(2).atan()
}

fn bark_to_hz(bark: f32) -> f32 {
    // Approximate inverse
    1960.0 * (bark + 0.53) / (26.28 - bark)
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
