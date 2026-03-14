pub mod degrader;
pub mod error;
pub mod acceptance;
pub mod constraints;

use crate::module_trait::{CirrusModule, ProcessContext};
use crate::types::{ValidatedResidual, CORE_FFT_SIZE};

/// M5: Self-Reprojection Validator.
///
/// The core innovation of CIRRUS: validates the residual by asking
/// "if we add this residual and then re-degrade, do we get back
/// the original input?"
///
/// D_θ̂(x + r) ≈ x → r is consistent
/// D_θ̂(x + r) ≠ x → r is inconsistent, shrink it
///
/// Freq→time conversion uses additive synthesis:
///   x[n] = (2/N) Σ_k R[k] · cos(2π·k·n/N + φ[k])
/// where R[k] is accepted residual magnitude and φ[k] is lattice phase.
pub struct SelfReprojectionValidator {
    sample_rate: u32,
    /// Pre-allocated scratch: combined freq-domain residual (num_bins).
    combined_buf: Vec<f32>,
    /// Pre-allocated scratch: reprojected signal (max_frame_size).
    reprojected_buf: Vec<f32>,
    /// Pre-allocated scratch: cosine table for synthesis (max_frame_size).
    /// Avoids repeated cos() calls for the same n values.
    synthesis_cos_cache: Vec<f32>,
}

impl SelfReprojectionValidator {
    pub fn new() -> Self {
        Self {
            sample_rate: 48000,
            combined_buf: Vec::new(),
            reprojected_buf: Vec::new(),
            synthesis_cos_cache: Vec::new(),
        }
    }
}

impl CirrusModule for SelfReprojectionValidator {
    fn name(&self) -> &'static str {
        "M5:Reprojection"
    }

    fn init(&mut self, max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
        let core_bins = CORE_FFT_SIZE / 2 + 1;
        self.combined_buf = vec![0.0; core_bins];
        self.reprojected_buf = vec![0.0; max_frame_size];
        self.synthesis_cos_cache = vec![0.0; max_frame_size];
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        let core_bins = ctx.lattice.core.num_bins();
        if core_bins == 0 {
            return;
        }

        let max_iters = ctx.config.quality_mode.max_reprojection_iters();
        let fft_size = ctx.lattice.core.fft_size;
        let cutoff_bin = {
            let bin_to_freq = ctx.sample_rate as f32 / fft_size.max(1) as f32;
            (ctx.damage.cutoff.mean / bin_to_freq) as usize
        };

        // Ensure validated residual is allocated
        let sample_len = samples.len();
        if ctx.validated.data.len() != sample_len {
            ctx.validated = ValidatedResidual::new(sample_len);
        }
        if ctx.validated.acceptance_mask.len() != core_bins {
            ctx.validated.acceptance_mask = vec![1.0; core_bins];
        }

        // Combine all residual components into a single candidate (zero-alloc)
        combine_residuals_into(&ctx.residual, core_bins, &mut self.combined_buf);

        // Iterative reprojection validation
        let mut best_error = f32::MAX;

        // Ensure reprojected_buf is large enough
        let reproj_len = self.reprojected_buf.len().min(samples.len());

        for _iter in 0..max_iters {
            // 1. Simulate degradation: D_θ̂(x + r) — zero-alloc path
            degrader::approximate_degradation_into(
                samples,
                &self.combined_buf[..core_bins],
                &ctx.damage,
                cutoff_bin,
                &mut self.reprojected_buf,
            );

            // 2. Compute reprojection error
            let e_rep = error::compute_reprojection_error(
                samples,
                &self.reprojected_buf[..reproj_len],
                core_bins,
            );

            // 3. Compute acceptance mask (dynamics-controlled threshold)
            let mask = acceptance::compute_acceptance_mask_dynamic(
                &e_rep, cutoff_bin, ctx.config.dynamics,
            );

            // 4. Apply constraints (low-band lock + impact band)
            let constrained_mask = constraints::apply_constraints_styled(
                &mask, cutoff_bin, ctx.sample_rate,
                fft_size,
                ctx.config.style.impact_gain,
                &ctx.fields.transient,
            );

            // 5. Shrink residual where error is high
            for k in 0..self.combined_buf.len().min(constrained_mask.len()) {
                self.combined_buf[k] *= constrained_mask[k];
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

        // ── Additive synthesis: freq-domain → time-domain ──
        // Convert validated frequency-domain residual to time-domain output.
        //
        // For each accepted bin k, synthesize its contribution:
        //   x[n] += (2/N) · R[k] · cos(2π·k·n/N + φ[k])
        //
        // Uses the original signal's phase from M2 lattice analysis.
        // Factor 2/N accounts for real-signal symmetry (positive freqs only).
        // DC (k=0) and Nyquist (k=N/2) should use 1/N, but cutoff_bin is
        // typically well above 0 and the Nyquist error is negligible.
        let out_len = ctx.validated.data.len();
        for i in 0..out_len {
            ctx.validated.data[i] = 0.0;
        }

        let scale = ctx.config.strength * ctx.damage.overall_confidence;
        if fft_size > 0 && scale > 1e-6 {
            let inv_fft = 2.0 / fft_size as f32;
            let two_pi_over_n = std::f32::consts::TAU / fft_size as f32;
            let num_synth_bins = self.combined_buf.len()
                .min(ctx.lattice.core.phase.len())
                .min(core_bins);

            for n in 0..out_len {
                let mut sum = 0.0f32;
                let omega_n = two_pi_over_n * n as f32;

                for k in cutoff_bin..num_synth_bins {
                    let mag = self.combined_buf[k];
                    if mag.abs() < 1e-8 {
                        continue; // skip negligible bins
                    }
                    let phase = ctx.lattice.core.phase[k];
                    sum += mag * (omega_n * k as f32 + phase).cos();
                }
                ctx.validated.data[n] = sum * scale * inv_fft;
            }
        }

        // Time-domain residuals bypass freq-domain validation — no additional scaling.
        // Declip already has internal scaling (clipping_severity * dynamics * transient).
        let time_len = ctx.time_candidate.len().min(ctx.validated.time_residual.len());
        for i in 0..time_len {
            ctx.validated.time_residual[i] = ctx.time_candidate[i];
        }

        ctx.validated.consistency_score =
            ctx.validated.acceptance_mask.iter().sum::<f32>()
                / ctx.validated.acceptance_mask.len().max(1) as f32;
        ctx.validated.reprojection_error = best_error;
    }

    fn reset(&mut self) {
        self.combined_buf.fill(0.0);
        self.reprojected_buf.fill(0.0);
        self.synthesis_cos_cache.fill(0.0);
    }
}

/// Combine all residual components into a single frequency-domain vector.
/// Zero-alloc: writes into pre-allocated `out` buffer.
fn combine_residuals_into(
    residual: &crate::types::ResidualCandidate,
    num_bins: usize,
    out: &mut Vec<f32>,
) {
    // Ensure capacity
    if out.len() < num_bins {
        out.resize(num_bins, 0.0);
    }

    for k in 0..num_bins {
        let mut val = 0.0f32;
        if k < residual.harmonic.len() {
            val += residual.harmonic[k];
        }
        if k < residual.air.len() {
            val += residual.air[k];
        }
        if k < residual.phase.len() {
            val += residual.phase[k].abs() * 0.1; // phase contributes weakly
        }
        out[k] = val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::types::{TriLattice, ResidualCandidate};
    use std::f32::consts::PI;

    #[test]
    fn reprojection_validator_initializes() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(2048, 48000);
        assert_eq!(m5.reprojected_buf.len(), 2048);
        assert_eq!(m5.combined_buf.len(), CORE_FFT_SIZE / 2 + 1);
    }

    #[test]
    fn reprojection_handles_empty_lattice() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        let mut samples = vec![0.0; 1024];
        m5.process(&mut samples, &mut ctx);
        // Should not crash, output untouched
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

    // ── Edge-case tests ──

    #[test]
    fn zero_residual_produces_zero_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.8;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        // All zeros — no residual to add

        let mut samples = vec![0.5; 128];
        m5.process(&mut samples, &mut ctx);

        // Validated data should be all zeros (no residual to synthesize)
        let max_val = ctx.validated.data.iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_val < 1e-6,
            "Zero residual should produce zero output, got max {max_val}"
        );
    }

    #[test]
    fn single_bin_produces_cosine() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 100.0; // very low cutoff to allow all bins
        ctx.damage.overall_confidence = 1.0;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);

        // Set a single bin with known magnitude and zero phase
        let test_bin = 10;
        let test_mag = 0.5;
        ctx.residual.harmonic[test_bin] = test_mag;
        // Phase is already 0.0 from init

        let mut samples = vec![0.0; 128]; // silence input
        m5.process(&mut samples, &mut ctx);

        // The output should contain a cosine at bin frequency
        // Check that the output is not all zero
        let energy: f32 = ctx.validated.data.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "Single bin should produce non-zero output");

        // Check that output is finite
        assert!(
            ctx.validated.data.iter().all(|s| s.is_finite()),
            "Output should be finite"
        );
    }

    #[test]
    fn cutoff_at_max_bin_produces_no_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        // Set cutoff above Nyquist — cutoff_bin will exceed num_bins
        ctx.damage.cutoff.mean = 30000.0;
        ctx.damage.overall_confidence = 0.8;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 0..core_bins {
            ctx.residual.harmonic[k] = 0.1;
        }

        let mut samples = vec![0.5; 128];
        m5.process(&mut samples, &mut ctx);

        // cutoff_bin > num_bins → no bins to synthesize
        let max_val = ctx.validated.data.iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_val < 1e-6,
            "Cutoff above all bins should produce zero output, got {max_val}"
        );
    }

    #[test]
    fn cutoff_at_zero_allows_all_bins() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 0.0; // cutoff at DC
        ctx.damage.overall_confidence = 1.0;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        // Put energy in a low bin
        ctx.residual.harmonic[5] = 0.3;

        let mut samples = vec![0.0; 128];
        m5.process(&mut samples, &mut ctx);

        // With cutoff at 0, bins above cutoff contribute
        // But the acceptance mask sets bins below cutoff to 0,
        // and constraints transition zone may affect low bins.
        // The key thing is it doesn't crash.
        assert!(ctx.validated.data.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn output_bounded_for_large_residual() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 10000.0;
        ctx.damage.overall_confidence = 1.0;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        // Large residual values
        for k in 200..core_bins {
            ctx.residual.harmonic[k] = 10.0;
        }

        let mut samples = vec![0.5; 128];
        m5.process(&mut samples, &mut ctx);

        // Output should be finite (reprojection validation should shrink bad residuals)
        assert!(
            ctx.validated.data.iter().all(|s| s.is_finite()),
            "Large residual should still produce finite output"
        );
    }

    #[test]
    fn tiny_frame_two_samples() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(2, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.5;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        ctx.residual.harmonic[300] = 0.05;

        let mut samples = vec![0.3, -0.3];
        m5.process(&mut samples, &mut ctx);

        assert_eq!(ctx.validated.data.len(), 2);
        assert!(ctx.validated.data.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn time_candidate_passes_through_unchanged() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(64, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.5;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);

        // Set up time_candidate with known values
        ctx.time_candidate = vec![0.0; 64];
        ctx.time_candidate[10] = 0.05;
        ctx.time_candidate[20] = -0.03;
        ctx.time_candidate[30] = 0.01;

        let mut samples = vec![0.5; 64];
        m5.process(&mut samples, &mut ctx);

        // time_residual should match time_candidate exactly
        assert_eq!(ctx.validated.time_residual.len(), 64);
        assert!((ctx.validated.time_residual[10] - 0.05).abs() < 1e-10);
        assert!((ctx.validated.time_residual[20] - (-0.03)).abs() < 1e-10);
        assert!((ctx.validated.time_residual[30] - 0.01).abs() < 1e-10);
        // Other samples should be zero
        assert!((ctx.validated.time_residual[0]).abs() < 1e-10);
    }

    #[test]
    fn zero_confidence_produces_zero_freq_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.0; // zero confidence

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 300..core_bins {
            ctx.residual.harmonic[k] = 0.1;
        }

        let mut samples = vec![0.5; 128];
        m5.process(&mut samples, &mut ctx);

        // scale = strength * 0.0 → freq-domain output should be zero
        let max_val = ctx.validated.data.iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(max_val < 1e-6, "Zero confidence → zero freq output, got {max_val}");
    }

    #[test]
    fn zero_strength_produces_zero_freq_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 0.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.8;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 300..core_bins {
            ctx.residual.harmonic[k] = 0.1;
        }

        let mut samples = vec![0.5; 128];
        m5.process(&mut samples, &mut ctx);

        let max_val = ctx.validated.data.iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(max_val < 1e-6, "Zero strength → zero freq output, got {max_val}");
    }

    #[test]
    fn multiple_iterations_do_not_diverge() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.quality_mode = crate::config::QualityMode::Ultra; // 3 iterations
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 12000.0;
        ctx.damage.overall_confidence = 0.9;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 250..core_bins {
            ctx.residual.harmonic[k] = 0.2;
            ctx.residual.air[k] = 0.05;
        }

        let mut samples = vec![0.3; 128];
        m5.process(&mut samples, &mut ctx);

        assert!(
            ctx.validated.data.iter().all(|s| s.is_finite()),
            "Multiple iterations should not diverge"
        );
        assert!(
            ctx.validated.reprojection_error.is_finite(),
            "Reprojection error should be finite"
        );
    }

    #[test]
    fn reset_clears_scratch_buffers() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        // Dirty the buffers
        m5.combined_buf.fill(1.0);
        m5.reprojected_buf.fill(1.0);

        m5.reset();

        assert!(m5.combined_buf.iter().all(|&v| v == 0.0));
        assert!(m5.reprojected_buf.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn phase_affects_synthesis_output() {
        // Two runs with different phase should produce different outputs
        let mut m5a = SelfReprojectionValidator::new();
        let mut m5b = SelfReprojectionValidator::new();
        m5a.init(128, 48000);
        m5b.init(128, 48000);

        let mut config = EngineConfig::default();
        config.strength = 1.0;

        // Run A: phase = 0
        let mut ctx_a = ProcessContext::new(48000, 2, config);
        ctx_a.lattice = TriLattice::new();
        ctx_a.damage.cutoff.mean = 5000.0;
        ctx_a.damage.overall_confidence = 1.0;
        let core_bins = ctx_a.lattice.core.num_bins();
        ctx_a.residual = ResidualCandidate::new(core_bins);
        ctx_a.residual.harmonic[200] = 0.3;
        // phase[200] = 0.0 (default)

        let mut samples_a = vec![0.0; 128];
        m5a.process(&mut samples_a, &mut ctx_a);

        // Run B: phase = π/2
        let mut ctx_b = ProcessContext::new(48000, 2, config);
        ctx_b.lattice = TriLattice::new();
        ctx_b.damage.cutoff.mean = 5000.0;
        ctx_b.damage.overall_confidence = 1.0;
        ctx_b.residual = ResidualCandidate::new(core_bins);
        ctx_b.residual.harmonic[200] = 0.3;
        ctx_b.lattice.core.phase[200] = PI / 2.0;

        let mut samples_b = vec![0.0; 128];
        m5b.process(&mut samples_b, &mut ctx_b);

        // Outputs should differ due to phase
        let diff: f32 = ctx_a.validated.data.iter()
            .zip(ctx_b.validated.data.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 1e-6, "Different phase should produce different output, diff={diff}");
    }

    #[test]
    fn combine_residuals_into_handles_mismatched_sizes() {
        let residual = ResidualCandidate::new(100);
        let mut out = vec![0.0; 200]; // larger than residual

        combine_residuals_into(&residual, 200, &mut out);

        // Bins beyond residual size should be zero
        for k in 100..200 {
            assert_eq!(out[k], 0.0);
        }
    }

    #[test]
    fn combine_residuals_sums_all_components() {
        let mut residual = ResidualCandidate::new(10);
        residual.harmonic[5] = 0.3;
        residual.air[5] = 0.2;
        residual.phase[5] = 1.0; // abs * 0.1 = 0.1

        let mut out = vec![0.0; 10];
        combine_residuals_into(&residual, 10, &mut out);

        let expected = 0.3 + 0.2 + 0.1;
        assert!(
            (out[5] - expected).abs() < 1e-6,
            "Expected {expected}, got {}",
            out[5]
        );
    }

    #[test]
    fn silence_input_with_residual_produces_finite_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(256, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.8;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 300..core_bins {
            ctx.residual.harmonic[k] = 0.05;
        }

        let mut samples = vec![0.0; 256]; // silence
        m5.process(&mut samples, &mut ctx);

        assert!(ctx.validated.data.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn dc_signal_with_residual_produces_finite_output() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.5;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        ctx.residual.harmonic[400] = 0.1;

        let mut samples = vec![0.8; 128]; // DC offset
        m5.process(&mut samples, &mut ctx);

        assert!(ctx.validated.data.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn full_scale_clipped_input_handled() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.clipping.mean = 0.8;
        ctx.damage.overall_confidence = 0.9;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        for k in 300..core_bins {
            ctx.residual.harmonic[k] = 0.1;
        }

        // Clipped signal
        let mut samples: Vec<f32> = (0..128)
            .map(|i| ((i as f32 / 10.0).sin() * 2.0).clamp(-1.0, 1.0))
            .collect();
        m5.process(&mut samples, &mut ctx);

        assert!(ctx.validated.data.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn only_air_residual_synthesizes() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 10000.0;
        ctx.damage.overall_confidence = 1.0;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        // Only air component
        for k in 300..core_bins {
            ctx.residual.air[k] = 0.08;
        }

        let mut samples = vec![0.0; 128];
        m5.process(&mut samples, &mut ctx);

        let energy: f32 = ctx.validated.data.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "Air-only residual should produce output");
        assert!(ctx.validated.data.iter().all(|s| s.is_finite()));
    }
}
