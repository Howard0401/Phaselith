pub mod harmonic;
pub mod transient;
pub mod air;
pub mod spatial;

use crate::module_trait::{CirrusModule, ProcessContext};
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
}

impl StructuredFactorizer {
    pub fn new() -> Self {
        Self {
            num_bins: 0,
            sample_rate: 48000,
        }
    }
}

impl CirrusModule for StructuredFactorizer {
    fn name(&self) -> &'static str {
        "M3:Factorizer"
    }

    fn init(&mut self, _max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        let core_bins = ctx.lattice.core.num_bins();
        if core_bins == 0 {
            return;
        }

        self.num_bins = core_bins;

        // Ensure fields are allocated
        if ctx.fields.harmonic.len() != core_bins {
            ctx.fields = StructuredFields::new(core_bins);
        }

        let bin_to_freq = ctx.sample_rate as f32 / ctx.lattice.core.fft_size as f32;

        // Harmonic ridge detection
        let (f0, ridge_score) =
            harmonic::detect_ridges(&ctx.lattice.core.magnitude, bin_to_freq, &mut ctx.fields.harmonic);
        ctx.fields.fundamental_freq = f0;
        ctx.fields.ridge_score = ridge_score;

        // Transient detection from micro lattice
        transient::detect_transients(
            &ctx.lattice.micro.energy,
            &ctx.lattice.core.energy,
            &mut ctx.fields.transient,
        );

        // Pre-echo suppression scaled by config.transient
        if ctx.config.transient > 0.01 {
            transient::suppress_pre_echo(samples, ctx.config.transient);
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
    }

    fn reset(&mut self) {
        self.num_bins = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::types::TriLattice;
    use crate::modules::m2_lattice::stft;

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
}
