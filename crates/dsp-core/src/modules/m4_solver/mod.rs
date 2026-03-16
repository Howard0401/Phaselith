pub mod air_cont;
pub mod declip;
pub mod harmonic_ext;
pub mod phase_relax;
pub mod side_recovery;

use crate::module_trait::{PhaselithModule, ProcessContext};
use crate::types::ResidualCandidate;

/// M4: Inverse Residual Solver.
///
/// Computes the "missing residual" — only what needs to be added
/// to restore the signal. Does not modify the original.
///
/// Sub-solvers:
/// - Declip back-projection
/// - Harmonic continuation above cutoff
/// - Air-field continuation
/// - Phase-field relaxation
/// - Bounded stereo side recovery
pub struct InverseResidualSolver {
    num_bins: usize,
    sample_rate: u32,
}

impl InverseResidualSolver {
    pub fn new() -> Self {
        Self {
            num_bins: 0,
            sample_rate: 48000,
        }
    }
}

impl PhaselithModule for InverseResidualSolver {
    fn name(&self) -> &'static str {
        "M4:Solver"
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
        let bin_to_freq = ctx.sample_rate as f32 / ctx.lattice.core.fft_size as f32;
        let cutoff_bin = (ctx.damage.cutoff.mean / bin_to_freq) as usize;

        // Ensure residual is allocated
        if ctx.residual.harmonic.len() != core_bins {
            // native-rt: only happens on first call if test bypasses engine builder
            ctx.residual = ResidualCandidate::new(core_bins);
        }
        ctx.residual.clear();

        // Ensure time_candidate is sized to sample count (not bin count)
        let sample_len = samples.len();
        if ctx.time_candidate.len() < sample_len {
            // native-rt: pre-allocated to max_frame_size in engine build(),
            // so this branch only fires if test bypasses engine builder.
            ctx.time_candidate.resize(sample_len, 0.0);
        }
        // Zero only the portion we'll use
        ctx.time_candidate[..sample_len].fill(0.0);

        let strength = ctx.config.strength;

        // 1. Declip (scaled by dynamics and transient params)
        // Writes time-domain per-sample corrections into time_candidate,
        // NOT into freq-domain residual — domain separation.
        if ctx.damage.clipping.mean > 0.05 {
            declip::compute_declip_residual_scaled(
                samples,
                ctx.damage.clipping.mean,
                ctx.config.dynamics,
                (ctx.config.transient * ctx.config.declip_transient_scaling).clamp(0.0, 1.0),
                &mut ctx.time_candidate,
            );
        }

        // 2. Harmonic continuation above cutoff + body reinforcement
        let style = &ctx.config.style;
        if cutoff_bin < core_bins && ctx.damage.cutoff.mean < 19500.0 {
            harmonic_ext::compute_harmonic_extension_styled(
                &ctx.lattice.core.magnitude,
                &ctx.lattice.core.phase,
                &ctx.fields.harmonic,
                cutoff_bin,
                bin_to_freq,
                strength * ctx.config.hf_reconstruction,
                style.air_brightness,
                style.body,
                &mut ctx.residual.harmonic,
            );
        } else {
            // Even without cutoff, apply body reinforcement for high-quality sources
            harmonic_ext::compute_harmonic_extension_styled(
                &ctx.lattice.core.magnitude,
                &ctx.lattice.core.phase,
                &ctx.fields.harmonic,
                core_bins, // no cutoff
                bin_to_freq,
                strength,
                style.air_brightness,
                style.body,
                &mut ctx.residual.harmonic,
            );
        }

        // 3. Air-field continuation
        if cutoff_bin < core_bins && ctx.damage.cutoff.mean < 19500.0 {
            air_cont::compute_air_continuation(
                &ctx.fields.air,
                cutoff_bin,
                strength * ctx.config.hf_reconstruction,
                &mut ctx.residual.air,
            );
        }

        // 4. Phase relaxation
        phase_relax::compute_phase_residual(
            &ctx.lattice.core.phase,
            cutoff_bin,
            &mut ctx.residual.phase,
        );

        // 5. Side recovery (with spatial_spread from style)
        // Use stereo-biased variant when cross-channel context is available
        if ctx.channels >= 2 && ctx.damage.stereo_collapse.mean > 0.1 {
            if let Some(ref cc) = ctx.cross_channel {
                side_recovery::compute_side_residual_stereo_biased(
                    &ctx.fields.spatial,
                    cc,
                    ctx.damage.stereo_collapse.mean,
                    strength,
                    style.spatial_spread,
                    &mut ctx.residual.side,
                );
            } else {
                // Fallback: no cross-channel info (mono mode or first frame)
                side_recovery::compute_side_residual_styled(
                    &ctx.fields.spatial,
                    ctx.damage.stereo_collapse.mean,
                    strength,
                    style.spatial_spread,
                    &mut ctx.residual.side,
                );
            }
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
    #[allow(unused_imports)]
    use crate::types::TriLattice;

    #[test]
    fn solver_initializes() {
        let mut m4 = InverseResidualSolver::new();
        m4.init(2048, 48000);
        assert_eq!(m4.sample_rate, 48000);
    }

    #[test]
    fn solver_skips_empty_lattice() {
        let mut m4 = InverseResidualSolver::new();
        m4.init(2048, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        // lattice is default (empty)
        let mut samples = vec![0.0; 1024];
        m4.process(&mut samples, &mut ctx);
        // Should not crash
    }
}
