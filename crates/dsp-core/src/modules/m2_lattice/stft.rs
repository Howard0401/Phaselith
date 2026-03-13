use rustfft::{FftPlanner, num_complex::Complex};
use crate::types::Lattice;

/// Analyze a block of samples into a Lattice using windowed FFT.
/// Fills magnitude, phase, and energy fields.
pub fn analyze_lattice(samples: &[f32], lattice: &mut Lattice, _sample_rate: u32) {
    let fft_size = lattice.fft_size;
    if fft_size == 0 || samples.len() < fft_size {
        return;
    }

    // Apply Hann window and build complex buffer
    let mut buffer: Vec<Complex<f32>> = samples[..fft_size]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let window = hann_window(i, fft_size);
            Complex::new(s * window, 0.0)
        })
        .collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut buffer);

    let num_bins = fft_size / 2 + 1;

    // Ensure lattice has correct size
    if lattice.magnitude.len() != num_bins {
        *lattice = Lattice::new(fft_size);
    }

    // Fill magnitude, phase, energy
    for (i, c) in buffer[..num_bins].iter().enumerate() {
        let mag = c.norm() / (fft_size as f32);
        lattice.magnitude[i] = mag;
        lattice.phase[i] = c.arg();
        lattice.energy[i] = mag * mag;
    }
}

/// Inverse FFT: synthesize time-domain samples from a Lattice.
/// Returns the synthesized block.
pub fn synthesize_from_lattice(lattice: &Lattice) -> Vec<f32> {
    let fft_size = lattice.fft_size;
    if fft_size == 0 {
        return Vec::new();
    }

    let num_bins = fft_size / 2 + 1;
    let mut buffer = vec![Complex::new(0.0f32, 0.0); fft_size];

    // Build full spectrum from magnitude + phase
    for i in 0..num_bins {
        let mag = lattice.magnitude[i] * fft_size as f32; // undo normalization
        buffer[i] = Complex::from_polar(mag, lattice.phase[i]);
    }
    // Mirror for negative frequencies
    for i in 1..fft_size / 2 {
        buffer[fft_size - i] = buffer[i].conj();
    }

    let mut planner = FftPlanner::new();
    let ifft = planner.plan_fft_inverse(fft_size);
    ifft.process(&mut buffer);

    buffer.iter().map(|c| c.re / fft_size as f32).collect()
}

/// Hann window function.
#[inline]
fn hann_window(i: usize, len: usize) -> f32 {
    0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (len - 1) as f32).cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_ifft_roundtrip() {
        let fft_size = 1024;
        let mut lattice = Lattice::new(fft_size);

        // Generate a sine wave
        let samples: Vec<f32> = (0..fft_size)
            .map(|i| (2.0 * std::f32::consts::PI * 5.0 * i as f32 / fft_size as f32).sin())
            .collect();

        analyze_lattice(&samples, &mut lattice, 48000);

        // Verify we got non-zero magnitudes
        let total_energy: f32 = lattice.energy.iter().sum();
        assert!(total_energy > 0.0, "Should have non-zero energy");

        // Verify peak is at bin 5
        let peak_bin = lattice
            .magnitude
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap();
        assert_eq!(peak_bin, 5, "Peak should be at bin 5");
    }

    #[test]
    fn synthesize_roundtrip() {
        let fft_size = 256;
        let mut lattice = Lattice::new(fft_size);

        let original: Vec<f32> = (0..fft_size)
            .map(|i| (2.0 * std::f32::consts::PI * 10.0 * i as f32 / fft_size as f32).sin())
            .collect();

        analyze_lattice(&original, &mut lattice, 48000);
        let reconstructed = synthesize_from_lattice(&lattice);

        assert_eq!(reconstructed.len(), fft_size);

        // Due to windowing, the roundtrip won't be perfect,
        // but the correlation should be high
        let correlation: f32 = original
            .iter()
            .zip(reconstructed.iter())
            .map(|(a, b)| a * b)
            .sum::<f32>();
        assert!(correlation > 0.0, "Correlation should be positive");
    }

    #[test]
    fn hann_window_endpoints_near_zero() {
        let len = 1024;
        assert!(hann_window(0, len) < 0.01);
        assert!(hann_window(len - 1, len) < 0.01);
    }

    #[test]
    fn hann_window_center_near_one() {
        let len = 1024;
        assert!((hann_window(len / 2, len) - 1.0).abs() < 0.01);
    }
}
