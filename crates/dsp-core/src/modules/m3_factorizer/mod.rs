pub mod air;
pub mod harmonic;
pub mod spatial;
pub mod transient;

use crate::module_trait::{PhaselithModule, ProcessContext};
use crate::types::StructuredFields;

/// M3: Structured Factorizer.
///
/// Decomposes the signal into four fields:
/// - Harmonic: tonal content (ridges at f0 multiples)
/// - Transient: onset/attack energy
/// - Air: stochastic high-frequency content
/// - Spatial: mid/side consistency
pub struct StructuredFactorizer {
    num_bins: usize,
    sample_rate: u32,
    /// Previous frame's core magnitude spectrum (for spectral flux computation).
    prev_magnitude: Vec<f32>,
    /// EMA-smoothed spectral flux (for adaptive thresholding).
    flux_ema: f32,
}

impl StructuredFactorizer {
    pub fn new() -> Self {
        Self {
            num_bins: 0,
            sample_rate: 48000,
            prev_magnitude: Vec::new(),
            flux_ema: 0.0,
        }
    }
}

impl PhaselithModule for StructuredFactorizer {
    fn name(&self) -> &'static str {
        "M3:Factorizer"
    }

    fn init(&mut self, _max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
        // NOTE: Do NOT pre-allocate prev_magnitude here.
        // If pre-filled with zeros, the first-frame spectral flux computation
        // sees a massive zero→signal jump, producing a false transient that
        // poisons the flux EMA and triggers spurious pre-echo suppression on
        // every subsequent block (SNR drops ~48 dB).
        // Leave empty so the first-frame guard (prev_magnitude.len() != mag.len())
        // skips flux and copies actual magnitude. Second frame onward is zero-alloc
        // via copy_from_slice since prev_magnitude is already correctly sized.
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        let core_bins = ctx.lattice.core.num_bins();
        if core_bins == 0 {
            return;
        }

        self.num_bins = core_bins;

        // Ensure fields are allocated
        // native-rt: pre-allocated in engine build(); this only fires
        // on first call if test bypasses engine builder.
        if ctx.fields.harmonic.len() != core_bins {
            ctx.fields = StructuredFields::new(core_bins);
        }

        let bin_to_freq = ctx.sample_rate as f32 / ctx.lattice.core.fft_size as f32;

        // Harmonic ridge detection
        let (f0, ridge_score) = harmonic::detect_ridges(
            &ctx.lattice.core.magnitude,
            bin_to_freq,
            &mut ctx.fields.harmonic,
        );
        ctx.fields.fundamental_freq = f0;
        ctx.fields.ridge_score = ridge_score;

        // Transient detection from micro lattice.
        // Only valid when both FFTs have comparable signal fill. When the
        // block is shorter than the core FFT (APO: 480 < 1024), the core
        // is heavily zero-padded while micro is fully filled, creating a
        // ~2× total energy imbalance that produces false peak_t=1.0 on
        // every frame. In that case, rely on spectral flux (below) instead.
        if samples.len() >= ctx.lattice.core.fft_size {
            transient::detect_transients(
                &ctx.lattice.micro.energy,
                &ctx.lattice.core.energy,
                &mut ctx.fields.transient,
            );
        }

        // Air field: high-frequency envelope from air lattice
        let air_bin_to_freq = ctx.sample_rate as f32 / ctx.lattice.air.fft_size as f32;
        let cutoff_bin = (ctx.damage.cutoff.mean / bin_to_freq) as usize;
        air::extract_air_field(
            &ctx.lattice.air.magnitude,
            air_bin_to_freq,
            cutoff_bin,
            &mut ctx.fields.air,
        );

        // Spatial analysis
        if ctx.channels >= 2 {
            spatial::analyze_spatial(
                samples,
                &ctx.lattice.core.magnitude,
                &mut ctx.fields.spatial,
            );
        }

        // ── Spectral flux transient detection ──
        // Computes L² norm of frame-to-frame magnitude change.
        // Uses adaptive threshold (EMA of flux) to flag transient frames.
        let mag = &ctx.lattice.core.magnitude;
        if !mag.is_empty() {
            if self.prev_magnitude.len() == mag.len() {
                // Compute spectral flux: sum of squared positive differences
                let mut flux = 0.0f32;
                let mut energy = 0.0f32;
                for k in 0..mag.len() {
                    let diff = mag[k] - self.prev_magnitude[k];
                    if diff > 0.0 {
                        flux += diff * diff; // only count increases (onsets, not offsets)
                    }
                    energy += mag[k] * mag[k];
                }
                // Normalize by total energy to get relative flux
                let norm_flux = if energy > 1e-10 {
                    (flux / energy).min(1.0)
                } else {
                    0.0
                };
                ctx.fields.spectral_flux = norm_flux;

                // Adaptive threshold: transient when flux > 3× running average
                let alpha = 0.05; // slow EMA (~20 frames)
                self.flux_ema += alpha * (norm_flux - self.flux_ema);
                ctx.fields.is_transient = norm_flux > self.flux_ema * 3.0 + 0.01;
            }

            // Store current magnitude for next frame comparison
            // native-rt: prev_magnitude is pre-allocated in init(), never clone
            if self.prev_magnitude.len() != mag.len() {
                #[cfg(not(feature = "native-rt"))]
                {
                    self.prev_magnitude = mag.clone();
                }
                #[cfg(feature = "native-rt")]
                {
                    // Should not happen — prev_magnitude pre-allocated to core_bins in init().
                    // Defensive: resize without cloning (fills with 0, next frame computes flux normally)
                    self.prev_magnitude.resize(mag.len(), 0.0);
                    self.prev_magnitude.copy_from_slice(mag);
                }
            } else {
                self.prev_magnitude.copy_from_slice(mag);
            }
        }

        if !ctx.config.delayed_transient_repair {
            let pre_echo_amount =
                (ctx.config.transient * ctx.config.pre_echo_transient_scaling).clamp(0.0, 1.0);
            if let Some(strength) = transient::pre_echo_strength(
                pre_echo_amount,
                ctx.hops_this_block,
                ctx.fields.is_transient,
                ctx.fields.spectral_flux,
                &ctx.fields.transient,
            ) {
                transient::suppress_pre_echo(samples, strength);
            }
        }
    }

    fn reset(&mut self) {
        self.num_bins = 0;
        self.prev_magnitude.clear();
        self.flux_ema = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::modules::m2_lattice::stft;
    use crate::types::TriLattice;

    #[test]
    fn factorizer_initializes() {
        let mut m3 = StructuredFactorizer::new();
        m3.init(2048, 48000);
        assert_eq!(m3.sample_rate, 48000);
    }

    #[test]
    fn factorizer_processes_with_lattice() {
        let mut m3 = StructuredFactorizer::new();
        m3.init(2048, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();

        // Generate 440 Hz sine and analyze
        let mut samples: Vec<f32> = (0..2048)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();

        // First run M2 to populate lattice
        stft::analyze_lattice(&samples[..1024], &mut ctx.lattice.core, 48000);
        stft::analyze_lattice(&samples[..256], &mut ctx.lattice.micro, 48000);
        stft::analyze_lattice(&samples[..2048], &mut ctx.lattice.air, 48000);

        m3.process(&mut samples, &mut ctx);

        // Should have allocated fields
        assert!(!ctx.fields.harmonic.is_empty());
    }

    #[test]
    fn factorizer_skips_pre_echo_without_hop_boundary() {
        let mut m3 = StructuredFactorizer::new();
        m3.init(128, 48000);
        let mut config = EngineConfig::default();
        config.transient = 1.0;
        let mut ctx = ProcessContext::new(48000, 1, config);
        ctx.lattice = TriLattice::new();
        ctx.hops_this_block = 0;
        ctx.lattice.micro.energy.fill(4.0);
        ctx.lattice.core.energy.fill(1.0);

        let original = vec![1.0f32; 128];
        let mut samples = original.clone();
        m3.process(&mut samples, &mut ctx);

        assert_eq!(
            samples, original,
            "No hop boundary should skip pre-echo shaping"
        );
    }

    #[test]
    fn factorizer_skips_pre_echo_without_transient_activity() {
        let mut m3 = StructuredFactorizer::new();
        m3.init(128, 48000);
        let mut config = EngineConfig::default();
        config.transient = 1.0;
        let mut ctx = ProcessContext::new(48000, 1, config);
        ctx.lattice = TriLattice::new();
        ctx.hops_this_block = 1;
        ctx.lattice.micro.energy.fill(1.0);
        ctx.lattice.core.energy.fill(1.0);

        let original = vec![1.0f32; 128];
        let mut samples = original.clone();
        m3.process(&mut samples, &mut ctx);

        assert_eq!(
            samples, original,
            "Steady-state blocks should remain untouched"
        );
    }

    #[test]
    fn factorizer_applies_pre_echo_only_for_transient_hops() {
        let mut m3 = StructuredFactorizer::new();
        m3.init(128, 48000);
        let mut config = EngineConfig::default();
        config.transient = 1.0;
        let mut ctx = ProcessContext::new(48000, 1, config);
        // Use block-sized lattices so detect_transients guard passes
        // (samples.len() >= core.fft_size). TriLattice::new() would set
        // core.fft_size=1024 which is > 128 and would skip detection.
        ctx.lattice.micro = crate::types::Lattice::new(128);
        ctx.lattice.core = crate::types::Lattice::new(128);
        ctx.lattice.air = crate::types::Lattice::new(128);
        ctx.hops_this_block = 1;
        ctx.lattice.micro.energy.fill(4.0);
        ctx.lattice.core.energy.fill(1.0);

        let mut samples = vec![1.0f32; 128];
        m3.process(&mut samples, &mut ctx);

        assert!(
            samples[0] < 0.8,
            "Transient hop should attenuate early samples"
        );
        assert!(
            samples[127] > 0.9,
            "Transient hop should preserve later samples"
        );
    }
}
