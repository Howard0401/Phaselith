mod fundamental;
mod decay;
mod classify;

use crate::stages::stage_trait::{DspStage, StageContext};
use crate::types::{FundamentalTrack, Harmonic, HarmonicMap};
use rustfft::{FftPlanner, num_complex::Complex};

/// Stage 2: Harmonic Tracking & Mapping.
///
/// Identifies fundamental frequencies and their harmonic series below
/// the cutoff frequency, fits decay curves, and builds a "harmonic map"
/// that Stage 3 uses for spectral reconstruction.
pub struct HarmonicTracker {
    fft_buffer: Vec<Complex<f32>>,
    magnitude_buf: Vec<f32>,
}

impl HarmonicTracker {
    pub fn new() -> Self {
        Self {
            fft_buffer: Vec::new(),
            magnitude_buf: Vec::new(),
        }
    }
}

impl DspStage for HarmonicTracker {
    fn name(&self) -> &'static str {
        "S2:Harmonics"
    }

    fn init(&mut self, max_frame_size: usize, _sample_rate: u32) {
        self.fft_buffer = vec![Complex::new(0.0, 0.0); max_frame_size];
        self.magnitude_buf = vec![0.0; max_frame_size / 2];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut StageContext) {
        let strength = ctx.config.hf_reconstruction * ctx.config.strength;
        if strength < 0.01 {
            return;
        }

        let cutoff = match ctx.degradation.cutoff_freq {
            Some(f) => f,
            None => return, // Lossless, no need to track
        };

        let fft_size = ctx.fft_size.min(samples.len()).min(self.fft_buffer.len());
        if fft_size < 64 {
            return;
        }

        // Window and FFT
        for i in 0..fft_size {
            let window = 0.5
                * (1.0
                    - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos());
            self.fft_buffer[i] = Complex::new(
                if i < samples.len() {
                    samples[i] * window
                } else {
                    0.0
                },
                0.0,
            );
        }

        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(fft_size);
        fft.process(&mut self.fft_buffer[..fft_size]);

        let half = fft_size / 2;
        for i in 0..half.min(self.magnitude_buf.len()) {
            self.magnitude_buf[i] = self.fft_buffer[i].norm();
        }

        let bin_to_freq = ctx.sample_rate as f32 / fft_size as f32;

        // Find spectral peaks (fundamental candidates)
        let peaks = fundamental::find_spectral_peaks(&self.magnitude_buf[..half], bin_to_freq);

        let mut tracks = Vec::new();

        for peak in &peaks {
            let f0 = peak.freq;
            if f0 < 50.0 || f0 > cutoff {
                continue;
            }

            let mut harmonics = Vec::new();
            let mut n = 1usize;

            // Estimate noise floor for this region
            let noise_floor = estimate_local_noise_floor(&self.magnitude_buf[..half]);

            while f0 * n as f32 <= cutoff {
                let harmonic_freq = f0 * n as f32;
                let bin = (harmonic_freq / bin_to_freq) as usize;
                if bin >= half {
                    break;
                }

                let mag = self.magnitude_buf[bin];
                let phase = self.fft_buffer[bin].arg();

                if mag > noise_floor * 3.0 {
                    harmonics.push(Harmonic {
                        order: n,
                        freq: harmonic_freq,
                        magnitude: mag,
                        phase,
                    });
                }
                n += 1;
            }

            // At least 3 harmonics for a valid track
            if harmonics.len() >= 3 {
                let decay_rate = decay::fit_decay_curve(&harmonics);
                let harmonic_type = classify::classify_type(&harmonics);
                tracks.push(FundamentalTrack {
                    freq: f0,
                    harmonics,
                    decay_rate,
                    harmonic_type,
                });
            }
        }

        // Extract noise envelope
        let noise_envelope = extract_noise_envelope(
            &self.magnitude_buf[..half],
            &tracks,
            bin_to_freq,
        );

        ctx.harmonic_map = HarmonicMap {
            tracks,
            noise_envelope,
        };
    }

    fn reset(&mut self) {
        self.fft_buffer.fill(Complex::new(0.0, 0.0));
        self.magnitude_buf.fill(0.0);
    }
}

fn estimate_local_noise_floor(magnitudes: &[f32]) -> f32 {
    if magnitudes.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f32> = magnitudes.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = sorted.len() / 10;
    sorted.get(idx).copied().unwrap_or(0.0)
}

fn extract_noise_envelope(
    magnitudes: &[f32],
    tracks: &[FundamentalTrack],
    bin_to_freq: f32,
) -> Vec<f32> {
    let mut envelope = magnitudes.to_vec();

    // Subtract harmonic peaks to get residual noise
    for track in tracks {
        for h in &track.harmonics {
            let bin = (h.freq / bin_to_freq) as usize;
            if bin < envelope.len() {
                // Zero out harmonic bins (±2 bins)
                let start = bin.saturating_sub(2);
                let end = (bin + 3).min(envelope.len());
                for b in start..end {
                    envelope[b] = 0.0;
                }
            }
        }
    }

    envelope
}
