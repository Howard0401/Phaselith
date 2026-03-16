mod extension;
mod noise_shape;
mod masking;

use crate::stages::stage_trait::{DspStage, StageContext};
use rustfft::{FftPlanner, num_complex::Complex};

/// Stage 3: Spectral Reconstruction.
///
/// Rebuilds high-frequency content above the detected cutoff using:
/// 1. Harmonic extension (extrapolate harmonic series decay curves)
/// 2. Noise shaping (add shaped noise for "air" and breathiness)
/// 3. Psychoacoustic masking constraint (don't add energy beyond masking threshold)
pub struct SpectralReconstructor {
    fft_buffer: Vec<Complex<f32>>,
    window: Vec<f32>,
    overlap_buffer: Vec<f32>,
    noise_phase: f32,
    /// Cached FFT plans — avoid FftPlanner::new() per frame
    cached_fft_fwd: Option<std::sync::Arc<dyn rustfft::Fft<f32>>>,
    cached_fft_inv: Option<std::sync::Arc<dyn rustfft::Fft<f32>>>,
    cached_fft_size: usize,
}

impl SpectralReconstructor {
    pub fn new() -> Self {
        Self {
            fft_buffer: Vec::new(),
            window: Vec::new(),
            overlap_buffer: Vec::new(),
            noise_phase: 0.0,
            cached_fft_fwd: None,
            cached_fft_inv: None,
            cached_fft_size: 0,
        }
    }
}

impl DspStage for SpectralReconstructor {
    fn name(&self) -> &'static str {
        "S3:Spectral"
    }

    fn init(&mut self, max_frame_size: usize, _sample_rate: u32) {
        self.fft_buffer = vec![Complex::new(0.0, 0.0); max_frame_size];
        self.window = (0..max_frame_size)
            .map(|i| {
                0.5 * (1.0
                    - (2.0 * std::f32::consts::PI * i as f32 / (max_frame_size - 1) as f32)
                        .cos())
            })
            .collect();
        self.overlap_buffer = vec![0.0; max_frame_size];
        // Pre-build FFT plans for max size
        let mut planner = FftPlanner::new();
        self.cached_fft_fwd = Some(planner.plan_fft_forward(max_frame_size));
        self.cached_fft_inv = Some(planner.plan_fft_inverse(max_frame_size));
        self.cached_fft_size = max_frame_size;
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut StageContext) {
        let strength = ctx.config.hf_reconstruction * ctx.config.strength;
        if strength < 0.01 {
            return;
        }

        let cutoff = match ctx.degradation.cutoff_freq {
            Some(f) => f,
            None => return,
        };

        let fft_size = ctx.fft_size.min(samples.len()).min(self.fft_buffer.len());
        if fft_size < 64 {
            return;
        }

        let bin_to_freq = ctx.sample_rate as f32 / fft_size as f32;
        let cutoff_bin = (cutoff / bin_to_freq) as usize;

        // Forward FFT
        for i in 0..fft_size {
            let w = if i < self.window.len() { self.window[i] } else { 0.0 };
            self.fft_buffer[i] = Complex::new(
                if i < samples.len() { samples[i] * w } else { 0.0 },
                0.0,
            );
        }

        // Reuse cached FFT plans; rebuild only if fft_size changed
        if self.cached_fft_size != fft_size || self.cached_fft_fwd.is_none() {
            let mut planner = FftPlanner::new();
            self.cached_fft_fwd = Some(planner.plan_fft_forward(fft_size));
            self.cached_fft_inv = Some(planner.plan_fft_inverse(fft_size));
            self.cached_fft_size = fft_size;
        }
        self.cached_fft_fwd.as_ref().unwrap().process(&mut self.fft_buffer[..fft_size]);

        // === Material 1: Harmonic extension ===
        extension::extend_harmonics(
            &mut self.fft_buffer[..fft_size / 2],
            &ctx.harmonic_map,
            cutoff,
            bin_to_freq,
            strength,
        );

        // === Material 2: Noise shaping ===
        self.noise_phase = noise_shape::add_shaped_noise(
            &mut self.fft_buffer[..fft_size / 2],
            &ctx.harmonic_map.noise_envelope,
            cutoff_bin,
            bin_to_freq,
            strength * 0.5,
            self.noise_phase,
        );

        // === Masking constraint ===
        masking::apply_masking_constraint(
            &mut self.fft_buffer[..fft_size / 2],
            cutoff_bin,
            bin_to_freq,
        );

        // Inverse FFT (uses cached plan)
        self.cached_fft_inv.as_ref().unwrap().process(&mut self.fft_buffer[..fft_size]);

        // Apply window and write back
        let norm = 1.0 / fft_size as f32;
        for i in 0..samples.len().min(fft_size) {
            let w = if i < self.window.len() { self.window[i] } else { 0.0 };
            samples[i] = self.fft_buffer[i].re * w * norm;
        }
    }

    fn reset(&mut self) {
        self.fft_buffer.fill(Complex::new(0.0, 0.0));
        self.overlap_buffer.fill(0.0);
        self.noise_phase = 0.0;
    }
}
