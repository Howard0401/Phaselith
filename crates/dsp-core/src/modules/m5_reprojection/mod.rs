pub mod degrader;
pub mod error;
pub mod acceptance;
pub mod constraints;

use crate::module_trait::{CirrusModule, ProcessContext};
use crate::types::ValidatedResidual;

/// M5: Self-Reprojection Validator.
///
/// The core innovation of CIRRUS: validates the residual by asking
/// "if we add this residual and then re-degrade, do we get back
/// the original input?"
///
/// D_θ̂(x + r) ≈ x → r is consistent
/// D_θ̂(x + r) ≠ x → r is inconsistent, shrink it
pub struct SelfReprojectionValidator {
    sample_rate: u32,
    /// Scratch buffer for reprojected signal.
    reprojected: Vec<f32>,
}

impl SelfReprojectionValidator {
    pub fn new() -> Self {
        Self {
            sample_rate: 48000,
            reprojected: Vec::new(),
        }
    }
}

impl CirrusModule for SelfReprojectionValidator {
    fn name(&self) -> &'static str {
        "M5:Reprojection"
    }

    fn init(&mut self, max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
        self.reprojected = vec![0.0; max_frame_size];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        let core_bins = ctx.lattice.core.num_bins();
        if core_bins == 0 {
            return;
        }

        let max_iters = ctx.config.quality_mode.max_reprojection_iters();
        let cutoff_bin = {
            let bin_to_freq = ctx.sample_rate as f32 / ctx.lattice.core.fft_size as f32;
            (ctx.damage.cutoff.mean / bin_to_freq) as usize
        };

        // Ensure validated residual is allocated
        if ctx.validated.data.len() != samples.len() {
            ctx.validated = ValidatedResidual::new(samples.len());
        }
        if ctx.validated.acceptance_mask.len() != core_bins {
            ctx.validated.acceptance_mask = vec![1.0; core_bins];
        }

        // Combine all residual components into a single candidate
        let combined_residual = combine_residuals(&ctx.residual, core_bins);

        // Iterative reprojection validation
        let mut current_residual = combined_residual;
        let mut best_error = f32::MAX;

        for _iter in 0..max_iters {
            // 1. Simulate degradation: D_θ̂(x + r)
            let reprojected = degrader::approximate_degradation(
                samples,
                &current_residual,
                &ctx.damage,
                cutoff_bin,
            );

            // 2. Compute reprojection error
            let e_rep = error::compute_reprojection_error(
                samples,
                &reprojected,
                core_bins,
            );

            // 3. Compute acceptance mask
            let mask = acceptance::compute_acceptance_mask(&e_rep, cutoff_bin);

            // 4. Apply constraints (low-band lock, etc.)
            let constrained_mask = constraints::apply_constraints(&mask, cutoff_bin);

            // 5. Shrink residual where error is high
            for k in 0..current_residual.len().min(constrained_mask.len()) {
                current_residual[k] *= constrained_mask[k];
            }

            // 6. Check convergence
            let j_rep: f32 = e_rep.iter().map(|e| e * e).sum::<f32>() / e_rep.len().max(1) as f32;
            if j_rep < best_error {
                best_error = j_rep;
            } else {
                break; // error increasing, stop
            }

            ctx.validated.acceptance_mask = constrained_mask;
        }

        // Write validated residual (time-domain approximation)
        // For now, copy frequency-domain residual scaled by mask into time-domain
        let out_len = ctx.validated.data.len();
        for i in 0..out_len {
            ctx.validated.data[i] = 0.0;
        }

        // Simple: add residual bins weighted by acceptance into output
        // Full implementation would use ISTFT
        let scale = ctx.config.strength * ctx.damage.overall_confidence;
        for k in cutoff_bin..current_residual.len().min(out_len) {
            // Approximate contribution from frequency bin to time domain
            // This is a placeholder; real implementation uses overlap-add ISTFT
            ctx.validated.data[k % out_len] += current_residual[k] * scale;
        }

        ctx.validated.consistency_score =
            ctx.validated.acceptance_mask.iter().sum::<f32>()
                / ctx.validated.acceptance_mask.len().max(1) as f32;
        ctx.validated.reprojection_error = best_error;
    }

    fn reset(&mut self) {
        self.reprojected.fill(0.0);
    }
}

/// Combine all residual components into a single frequency-domain vector.
fn combine_residuals(residual: &crate::types::ResidualCandidate, num_bins: usize) -> Vec<f32> {
    let mut combined = vec![0.0f32; num_bins];

    for k in 0..num_bins {
        if k < residual.harmonic.len() {
            combined[k] += residual.harmonic[k];
        }
        if k < residual.air.len() {
            combined[k] += residual.air[k];
        }
        if k < residual.phase.len() {
            combined[k] += residual.phase[k].abs() * 0.1; // phase contributes weakly
        }
    }

    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::types::{TriLattice, ResidualCandidate};

    #[test]
    fn reprojection_validator_initializes() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(2048, 48000);
        assert_eq!(m5.reprojected.len(), 2048);
    }

    #[test]
    fn reprojection_handles_empty_lattice() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        let mut samples = vec![0.0; 1024];
        m5.process(&mut samples, &mut ctx);
        // Should not crash
    }

    #[test]
    fn reprojection_produces_validated_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.8;

        // Set up some residual
        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 300..core_bins {
            ctx.residual.harmonic[k] = 0.1;
        }

        let mut samples = vec![0.5; 1024];
        m5.process(&mut samples, &mut ctx);

        assert!(
            ctx.validated.consistency_score >= 0.0,
            "Should have a consistency score"
        );
    }
}
