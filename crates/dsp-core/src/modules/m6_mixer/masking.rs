use rustfft::num_complex::Complex;
use crate::psychoacoustic;

/// Apply psychoacoustic masking constraint to prevent over-reconstruction.
/// Limits reconstructed content to stay below the perceptual masking curve.
///
/// Uses the shared `psychoacoustic` module for threshold computation,
/// which provides proper Schroeder spreading function instead of the
/// previous 6-point discrete Bark approximation.
pub fn apply_masking_constraint(
    spectrum: &mut [Complex<f32>],
    cutoff_bin: usize,
    bin_to_freq: f32,
) {
    // Extract magnitudes for threshold computation
    let magnitudes: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();

    for bin in cutoff_bin..spectrum.len() {
        let freq = bin as f32 * bin_to_freq;
        if freq > 22000.0 {
            break;
        }

        let threshold = psychoacoustic::masking_threshold(
            bin,
            bin_to_freq,
            &magnitudes,
            true, // use simultaneous masking
        );
        let current = magnitudes[bin];

        if current > threshold * 1.5 {
            spectrum[bin] *= threshold * 1.5 / current;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::psychoacoustic;

    #[test]
    fn hearing_threshold_u_shaped() {
        let t_500 = psychoacoustic::absolute_threshold_linear(500.0);
        let t_3000 = psychoacoustic::absolute_threshold_linear(3000.0);
        let t_15000 = psychoacoustic::absolute_threshold_linear(15000.0);

        // Hearing is most sensitive around 3-4 kHz
        assert!(t_3000 < t_500, "3kHz should be more sensitive than 500Hz");
        assert!(t_3000 < t_15000, "3kHz should be more sensitive than 15kHz");
    }

    #[test]
    fn bark_hz_roundtrip() {
        let freq = 1000.0;
        let bark = psychoacoustic::hz_to_bark(freq);
        let back = psychoacoustic::bark_to_hz(bark);
        assert!(
            (back - freq).abs() < 100.0,
            "Roundtrip should be close: {} vs {}",
            back,
            freq
        );
    }
}
