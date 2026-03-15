use rustfft::num_complex::Complex;

/// Apply psychoacoustic masking constraint to prevent over-reconstruction.
/// Limits reconstructed content to stay below the perceptual masking curve.
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

/// Compute masking threshold combining absolute hearing threshold
/// with simultaneous masking from nearby spectral energy.
fn masking_threshold(freq: f32, spectrum: &[Complex<f32>], bin_to_freq: f32) -> f32 {
    let absolute = absolute_hearing_threshold(freq);

    let bark = hz_to_bark(freq);
    let mut masking = 0.0f32;

    for offset in [-3.0f32, -2.0, -1.0, 1.0, 2.0, 3.0] {
        let neighbor_bark = bark + offset;
        let neighbor_freq = bark_to_hz(neighbor_bark);
        let neighbor_bin = (neighbor_freq / bin_to_freq) as usize;

        if neighbor_bin < spectrum.len() {
            let energy = spectrum[neighbor_bin].norm_sqr();
            let distance = offset.abs();
            let spread = if offset > 0.0 {
                (-3.0 * distance).exp() * energy
            } else {
                (-5.0 * distance).exp() * energy
            };
            masking = masking.max(spread.sqrt());
        }
    }

    let mask_offset = 0.5;
    absolute.max(masking * mask_offset)
}

/// Absolute hearing threshold (Terhardt approximation).
fn absolute_hearing_threshold(freq: f32) -> f32 {
    let f_khz = freq / 1000.0;
    let threshold_db =
        3.64 * f_khz.powf(-0.8) - 6.5 * (-0.6 * (f_khz - 3.3).powi(2)).exp() + 1e-3 * f_khz.powi(4);
    db_to_linear(threshold_db - 96.0)
}

fn hz_to_bark(freq: f32) -> f32 {
    13.0 * (0.00076 * freq).atan() + 3.5 * (freq / 7500.0).powi(2).atan()
}

fn bark_to_hz(bark: f32) -> f32 {
    1960.0 * (bark + 0.53) / (26.28 - bark)
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearing_threshold_u_shaped() {
        let t_500 = absolute_hearing_threshold(500.0);
        let t_3000 = absolute_hearing_threshold(3000.0);
        let t_15000 = absolute_hearing_threshold(15000.0);

        // Hearing is most sensitive around 3-4 kHz
        assert!(t_3000 < t_500, "3kHz should be more sensitive than 500Hz");
        assert!(t_3000 < t_15000, "3kHz should be more sensitive than 15kHz");
    }

    #[test]
    fn bark_hz_roundtrip() {
        let freq = 1000.0;
        let bark = hz_to_bark(freq);
        let back = bark_to_hz(bark);
        assert!(
            (back - freq).abs() < 100.0,
            "Roundtrip should be close: {} vs {}",
            back,
            freq
        );
    }
}
