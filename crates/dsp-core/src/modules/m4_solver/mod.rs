pub mod declip;
pub mod harmonic_ext;
pub mod air_cont;
pub mod phase_relax;
pub mod side_recovery;

use crate::module_trait::{CirrusModule, ProcessContext};
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

impl CirrusModule for InverseResidualSolver {
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
            ctx.residual = ResidualCandidate::new(core_bins);
        }
        ctx.residual.clear();

        let strength = ctx.config.strength;

        // 1. Declip
        if ctx.damage.clipping.mean > 0.05 {
            declip::compute_declip_residual(
                samples,
                ctx.damage.clipping.mean,
                &mut ctx.residual.transient,
            );
        }

        // 2. Harmonic continuation above cutoff
        if cutoff_bin < core_bins && ctx.damage.cutoff.mean < 19500.0 {
            harmonic_ext::compute_harmonic_extension(
                &ctx.lattice.core.magnitude,
                &ctx.lattice.core.phase,
                &ctx.fields.harmonic,
                cutoff_bin,
                bin_to_freq,
                strength * ctx.config.hf_reconstruction,
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

        // 5. Side recovery
        if ctx.channels >= 2 && ctx.damage.stereo_collapse.mean > 0.1 {
            side_recovery::compute_side_residual(
                &ctx.fields.spatial,
                ctx.damage.stereo_collapse.mean,
                strength,
                &mut ctx.residual.side,
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
