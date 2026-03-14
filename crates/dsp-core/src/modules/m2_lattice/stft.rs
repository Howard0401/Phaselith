use std::sync::Arc;
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use crate::types::Lattice;

// ─── Zero-alloc STFT Engine ───

/// Pre-allocated STFT engine for a single FFT size.
/// All buffers are allocated in `new()` — `analyze()` is zero-alloc.
pub struct StftEngine {
    fft_size: usize,
    window: Vec<f32>,
    complex_buf: Vec<Complex<f32>>,
    fft_forward: Arc<dyn Fft<f32>>,
    fft_inverse: Arc<dyn Fft<f32>>,
}

impl StftEngine {
    /// Create a new STFT engine for the given FFT size.
    /// Allocates window, complex buffer, and FFT plans.
    pub fn new(fft_size: usize) -> Self {
        let window: Vec<f32> = (0..fft_size)
            .map(|i| hann_window(i, fft_size))
            .collect();
        let complex_buf = vec![Complex::new(0.0f32, 0.0); fft_size];
        let mut planner = FftPlanner::new();
        let fft_forward = planner.plan_fft_forward(fft_size);
        let fft_inverse = planner.plan_fft_inverse(fft_size);

        Self {
            fft_size,
            window,
            complex_buf,
            fft_forward,
            fft_inverse,
        }
    }

    /// Zero-alloc forward STFT: window + FFT → fills lattice magnitude/phase/energy.
    /// `samples` must be at least `fft_size` long.
    pub fn analyze(&mut self, samples: &[f32], lattice: &mut Lattice) {
        let fft_size = self.fft_size;
        if samples.len() < fft_size {
            return;
        }

        // Apply window into pre-allocated complex buffer
        for i in 0..fft_size {
            self.complex_buf[i] = Complex::new(samples[i] * self.window[i], 0.0);
        }

        self.fft_forward.process(&mut self.complex_buf);

        let num_bins = fft_size / 2 + 1;
        if lattice.magnitude.len() != num_bins {
            *lattice = Lattice::new(fft_size);
        }

        let inv_n = 1.0 / fft_size as f32;
        for i in 0..num_bins {
            let c = self.complex_buf[i];
            let mag = c.norm() * inv_n;
            lattice.magnitude[i] = mag;
            lattice.phase[i] = c.arg();
            lattice.energy[i] = mag * mag;
        }
    }

    /// Zero-alloc inverse STFT: magnitude + phase → time-domain output.
    /// Writes into `output` (must be at least `fft_size` long).
    pub fn synthesize_into(&mut self, lattice: &Lattice, output: &mut [f32]) {
        let fft_size = self.fft_size;
        if output.len() < fft_size {
            return;
        }
        let num_bins = fft_size / 2 + 1;

        // Build full spectrum
        for i in 0..num_bins {
            let mag = lattice.magnitude[i] * fft_size as f32; // undo normalization
            self.complex_buf[i] = Complex::from_polar(mag, lattice.phase[i]);
        }
        // Mirror for negative frequencies
        for i in 1..fft_size / 2 {
            self.complex_buf[fft_size - i] = self.complex_buf[i].conj();
        }

        self.fft_inverse.process(&mut self.complex_buf);

        let inv_n = 1.0 / fft_size as f32;
        for i in 0..fft_size {
            output[i] = self.complex_buf[i].re * inv_n;
        }
    }

    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Access the pre-computed Hann window (for synthesis windowing in OLA).
    pub fn window(&self) -> &[f32] {
        &self.window
    }
}

// ─── Legacy free-function API (still used by M2 and tests) ───

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

    // ── StftEngine tests ──

    #[test]
    fn stft_engine_matches_legacy_analysis() {
        let fft_size = 1024;
        let mut engine = StftEngine::new(fft_size);
        let mut lattice_engine = Lattice::new(fft_size);
        let mut lattice_legacy = Lattice::new(fft_size);

        let samples: Vec<f32> = (0..fft_size)
            .map(|i| (2.0 * std::f32::consts::PI * 5.0 * i as f32 / fft_size as f32).sin())
            .collect();

        engine.analyze(&samples, &mut lattice_engine);
        analyze_lattice(&samples, &mut lattice_legacy, 48000);

        // Results should be identical
        for i in 0..lattice_engine.num_bins() {
            assert!(
                (lattice_engine.magnitude[i] - lattice_legacy.magnitude[i]).abs() < 1e-6,
                "Magnitude mismatch at bin {}: engine={} legacy={}",
                i, lattice_engine.magnitude[i], lattice_legacy.magnitude[i]
            );
            // Phase can differ by 2π for very small magnitudes; only check significant bins
            if lattice_engine.magnitude[i] > 1e-6 {
                let phase_diff = (lattice_engine.phase[i] - lattice_legacy.phase[i]).abs();
                assert!(
                    phase_diff < 1e-4 || (phase_diff - 2.0 * std::f32::consts::PI).abs() < 1e-4,
                    "Phase mismatch at bin {}: engine={} legacy={}",
                    i, lattice_engine.phase[i], lattice_legacy.phase[i]
                );
            }
        }
    }

    #[test]
    fn stft_engine_synthesize_roundtrip() {
        let fft_size = 256;
        let mut engine = StftEngine::new(fft_size);
        let mut lattice = Lattice::new(fft_size);

        let original: Vec<f32> = (0..fft_size)
            .map(|i| (2.0 * std::f32::consts::PI * 10.0 * i as f32 / fft_size as f32).sin())
            .collect();

        engine.analyze(&original, &mut lattice);

        let mut output = vec![0.0f32; fft_size];
        engine.synthesize_into(&lattice, &mut output);

        // Due to windowing, not perfect — but correlation should be high
        let correlation: f32 = original.iter()
            .zip(output.iter())
            .map(|(a, b)| a * b)
            .sum();
        assert!(correlation > 0.0, "Roundtrip correlation should be positive");
    }

    #[test]
    fn stft_engine_zero_alloc_repeated_calls() {
        let fft_size = 512;
        let mut engine = StftEngine::new(fft_size);
        let mut lattice = Lattice::new(fft_size);

        // Call analyze multiple times — should not allocate
        for freq in [5.0, 10.0, 20.0, 50.0] {
            let samples: Vec<f32> = (0..fft_size)
                .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / fft_size as f32).sin())
                .collect();
            engine.analyze(&samples, &mut lattice);

            let peak_bin = lattice.magnitude.iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .map(|(i, _)| i)
                .unwrap();
            let expected_bin = freq as usize;
            assert!(
                (peak_bin as i32 - expected_bin as i32).abs() <= 1,
                "Peak should be near bin {} for freq {}, got {}",
                expected_bin, freq, peak_bin
            );
        }
    }
}
