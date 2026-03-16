pub mod degrader;
pub mod error;
pub mod acceptance;
pub mod constraints;
pub mod synthesizer;
pub mod overlap_add;

use crate::fft::planner::SharedFftPlans;
use crate::frame::SynthesisMode;
use crate::module_trait::{PhaselithModule, ProcessContext};
use crate::modules::m2_lattice::stft::StftEngine;
use crate::types::{Lattice, ValidatedResidual, CORE_FFT_SIZE};

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
    /// StftEngine for pilot ISTFT synthesis (Phase B1-A/B).
    /// Uses the real IFFT path instead of sum-of-cosines.
    pilot_engine: Option<StftEngine>,
    /// Scratch Lattice for building ISTFT input from residual + original phase.
    pilot_lattice: Lattice,
    /// Scratch buffer for ISTFT output (fft_size samples).
    pilot_istft_buf: Vec<f32>,
    /// Pre-allocated scratch: reprojection error (core_bins) — native-rt only.
    #[cfg(feature = "native-rt")]
    error_scratch: Vec<f32>,
    /// Pre-allocated scratch: Wiener mask (core_bins) — native-rt only.
    #[cfg(feature = "native-rt")]
    mask_scratch: Vec<f32>,
    /// Pre-allocated scratch: constrained mask (core_bins) — native-rt only.
    #[cfg(feature = "native-rt")]
    constrained_scratch: Vec<f32>,
    /// Phase B1-B: OLA accumulator for ISTFT frames.
    /// Properly overlaps ISTFT frame tails across hop boundaries.
    ola_buffer: overlap_add::OverlapAddBuffer,
    /// Drain buffer: holds one hop of OLA output, drained block_size at a time.
    ola_drain: Vec<f32>,
    /// Current read position within ola_drain.
    ola_drain_pos: usize,
    /// Hop size used for OLA (cached from FrameParams).
    ola_hop_size: usize,
}

impl SelfReprojectionValidator {
    pub fn new() -> Self {
        let default_hop = 256;
        Self {
            sample_rate: 48000,
            combined_buf: Vec::new(),
            reprojected_buf: Vec::new(),
            synthesis_cos_cache: Vec::new(),
            pilot_engine: None,
            pilot_lattice: Lattice::new(CORE_FFT_SIZE),
            pilot_istft_buf: Vec::new(),
            #[cfg(feature = "native-rt")]
            error_scratch: Vec::new(),
            #[cfg(feature = "native-rt")]
            mask_scratch: Vec::new(),
            #[cfg(feature = "native-rt")]
            constrained_scratch: Vec::new(),
            ola_buffer: overlap_add::OverlapAddBuffer::new(CORE_FFT_SIZE, default_hop),
            ola_drain: vec![0.0; default_hop * 6],
            ola_drain_pos: 0, // start empty — outputs zeros until enough OLA frames accumulate
            ola_hop_size: default_hop,
        }
    }

    /// Initialize using a shared FFT plan cache for the pilot ISTFT engine.
    pub fn init_with_plans(&mut self, max_frame_size: usize, sample_rate: u32, plans: &mut SharedFftPlans) {
        self.sample_rate = sample_rate;
        let core_bins = CORE_FFT_SIZE / 2 + 1;
        self.combined_buf = vec![0.0; core_bins];
        self.reprojected_buf = vec![0.0; max_frame_size];
        self.synthesis_cos_cache = vec![0.0; max_frame_size];
        self.pilot_engine = Some(StftEngine::new_with_plans(plans, CORE_FFT_SIZE));
        self.pilot_lattice = Lattice::new(CORE_FFT_SIZE);
        self.pilot_istft_buf = vec![0.0; CORE_FFT_SIZE];
        #[cfg(feature = "native-rt")]
        {
            self.error_scratch = vec![0.0; core_bins];
            self.mask_scratch = vec![0.0; core_bins];
            self.constrained_scratch = vec![0.0; core_bins];
        }
        let hop = self.ola_hop_size;
        self.ola_buffer = overlap_add::OverlapAddBuffer::new(CORE_FFT_SIZE, hop);
        self.ola_drain = vec![0.0; hop * 6];
        self.ola_drain_pos = 0;
    }

    /// ISTFT one frame: builds a Lattice from residual magnitudes + analysis phase,
    /// runs IFFT via StftEngine, and writes the result to `istft_buf`.
    ///
    /// Does NOT write to output — caller decides how to deliver (truncation or OLA).
    ///
    /// Takes individual fields instead of &mut self to avoid borrow conflicts
    /// with combined_buf in the caller.
    fn istft_residual(
        engine: &mut StftEngine,
        lattice: &mut Lattice,
        istft_buf: &mut Vec<f32>,
        combined: &[f32],
        analysis_phase: &[f32],
        cutoff_bin: usize,
        fft_size: usize,
        scale: f32,
    ) -> bool {
        if fft_size != engine.fft_size() || scale < 1e-6 {
            return false;
        }

        let num_bins = fft_size / 2 + 1;

        // Ensure lattice is sized correctly
        // native-rt: pilot_lattice pre-allocated in init(); only fires
        // on first call if test bypasses engine builder.
        if lattice.magnitude.len() != num_bins {
            *lattice = Lattice::new(fft_size);
        }

        // Build lattice: residual magnitude (scaled) + original analysis phase.
        // Bins below cutoff are zeroed (low-band lock).
        for k in 0..num_bins {
            if k < cutoff_bin || k >= combined.len() || k >= analysis_phase.len() {
                lattice.magnitude[k] = 0.0;
                lattice.phase[k] = 0.0;
            } else {
                lattice.magnitude[k] = combined[k] * scale;
                lattice.phase[k] = analysis_phase[k];
            }
        }

        // ISTFT: freq-domain → time-domain via real IFFT
        // native-rt: pilot_istft_buf pre-allocated to CORE_FFT_SIZE in init()
        if istft_buf.len() < fft_size {
            istft_buf.resize(fft_size, 0.0);
        }
        engine.synthesize_into(lattice, istft_buf);

        true
    }
}

impl PhaselithModule for SelfReprojectionValidator {
    fn name(&self) -> &'static str {
        "M5:Reprojection"
    }

    fn init(&mut self, max_frame_size: usize, sample_rate: u32) {
        self.sample_rate = sample_rate;
        let core_bins = CORE_FFT_SIZE / 2 + 1;
        self.combined_buf = vec![0.0; core_bins];
        self.reprojected_buf = vec![0.0; max_frame_size];
        self.synthesis_cos_cache = vec![0.0; max_frame_size];
        // Phase B1-A/B: pre-allocate ISTFT engine + scratch + OLA
        self.pilot_engine = Some(StftEngine::new(CORE_FFT_SIZE));
        self.pilot_lattice = Lattice::new(CORE_FFT_SIZE);
        self.pilot_istft_buf = vec![0.0; CORE_FFT_SIZE];
        #[cfg(feature = "native-rt")]
        {
            self.error_scratch = vec![0.0; core_bins];
            self.mask_scratch = vec![0.0; core_bins];
            self.constrained_scratch = vec![0.0; core_bins];
        }
        // OLA buffer uses default hop (256). Updated in process() if FrameParams differs.
        let hop = self.ola_hop_size;
        self.ola_buffer = overlap_add::OverlapAddBuffer::new(CORE_FFT_SIZE, hop);
        self.ola_drain = vec![0.0; hop * 6];
        self.ola_drain_pos = 0; // start empty — outputs zeros until enough OLA frames accumulate
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
        // native-rt: pre-allocated in engine build(); this only fires
        // on first call if test bypasses engine builder.
        let sample_len = samples.len();
        if ctx.validated.data.len() < sample_len {
            ctx.validated = ValidatedResidual::new(sample_len);
        }
        if ctx.validated.acceptance_mask.len() < core_bins {
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

            #[cfg(feature = "native-rt")]
            {
                // 2. Compute reprojection error (zero-alloc)
                error::compute_reprojection_error_into(
                    samples,
                    &self.reprojected_buf[..reproj_len],
                    core_bins,
                    &mut self.error_scratch,
                );

                // 3. Compute Wiener soft mask (zero-alloc)
                acceptance::compute_wiener_mask_into(
                    &self.error_scratch[..core_bins],
                    &self.combined_buf[..core_bins],
                    cutoff_bin,
                    ctx.config.dynamics,
                    &mut self.mask_scratch,
                );

                // 4. Apply constraints (zero-alloc)
                constraints::apply_constraints_styled_into(
                    &self.mask_scratch[..core_bins],
                    cutoff_bin,
                    ctx.sample_rate,
                    fft_size,
                    ctx.config.style.impact_gain,
                    &ctx.fields.transient,
                    &mut self.constrained_scratch,
                );

                // 5. Shrink residual where error is high
                for k in 0..self.combined_buf.len().min(core_bins) {
                    self.combined_buf[k] *= self.constrained_scratch[k];
                }

                // 6. Check convergence
                let j_rep: f32 = self.error_scratch[..core_bins].iter().map(|e| e * e).sum::<f32>()
                    / core_bins.max(1) as f32;
                if j_rep < best_error {
                    best_error = j_rep;
                } else {
                    break;
                }

                // Copy constrained mask into acceptance_mask (zero-alloc)
                let copy_len = ctx.validated.acceptance_mask.len().min(core_bins);
                ctx.validated.acceptance_mask[..copy_len]
                    .copy_from_slice(&self.constrained_scratch[..copy_len]);
            }

            #[cfg(not(feature = "native-rt"))]
            {
                // 2. Compute reprojection error
                let e_rep = error::compute_reprojection_error(
                    samples,
                    &self.reprojected_buf[..reproj_len],
                    core_bins,
                );

                // 3. Compute Wiener soft mask (dynamics-controlled spectral floor)
                let mask = acceptance::compute_wiener_mask(
                    &e_rep,
                    &self.combined_buf[..core_bins],
                    cutoff_bin,
                    ctx.config.dynamics,
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
                    break;
                }

                ctx.validated.acceptance_mask = constrained_mask;
            }
        }

        // ── Synthesis: freq-domain → time-domain ──
        let scale = ctx.config.strength * ctx.damage.overall_confidence;
        let num_synth_bins = self.combined_buf.len()
            .min(ctx.lattice.core.phase.len())
            .min(core_bins);

        match ctx.synthesis_mode {
            SynthesisMode::FftOlaPilot => {
                // Phase B1-B: Pilot ISTFT + OLA with FIFO drain.
                //
                // The OLA drain is a FIFO that decouples hop-aligned synthesis
                // from arbitrary host block sizes. This is critical for APO where
                // block_size (480) != hop_size (256): the old code drained only
                // one hop (256 samples) per block, leaving 47% of output zeroed.
                //
                // New flow:
                // 1. At hop boundaries: ISTFT → window → add to OLA accumulator
                // 2. Drain ALL available hops from OLA into FIFO
                // 3. Serve block_size samples from FIFO to validated.data
                // 4. Compact FIFO (shift remaining data to front)
                let hop_size = ctx.frame_params.hop_size;
                let block_size = samples.len();

                // Lazily update OLA hop size if FrameParams changed
                if hop_size != self.ola_hop_size && hop_size > 0 {
                    self.ola_hop_size = hop_size;
                    #[cfg(not(feature = "native-rt"))]
                    {
                        self.ola_buffer = overlap_add::OverlapAddBuffer::new(fft_size, hop_size);
                        self.ola_drain = vec![0.0; hop_size * 6];
                        self.ola_drain_pos = 0;
                    }
                    #[cfg(feature = "native-rt")]
                    {
                        let needed = hop_size * 6;
                        if self.ola_drain.len() < needed {
                            self.ola_drain = vec![0.0; needed];
                        } else {
                            let clear_end = self.ola_drain_pos.min(self.ola_drain.len());
                            self.ola_drain[..clear_end].fill(0.0);
                        }
                        self.ola_buffer = overlap_add::OverlapAddBuffer::new(fft_size, hop_size);
                        self.ola_drain_pos = 0;
                    }
                }

                // Step 1: At hop boundaries, ISTFT + window + add to OLA
                if ctx.hops_this_block > 0 {
                    if let Some(engine) = &mut self.pilot_engine {
                        let ok = Self::istft_residual(
                            engine,
                            &mut self.pilot_lattice,
                            &mut self.pilot_istft_buf,
                            &self.combined_buf[..num_synth_bins],
                            &ctx.lattice.core.phase[..num_synth_bins],
                            cutoff_bin,
                            fft_size,
                            scale,
                        );

                        if ok {
                            // Apply synthesis Hann window for smooth OLA overlap
                            let window = engine.window();
                            let win_len = window.len().min(self.pilot_istft_buf.len());
                            for i in 0..win_len {
                                self.pilot_istft_buf[i] *= window[i];
                            }

                            // Add windowed frame to OLA accumulator for each hop.
                            // M2 produces one analysis per block, but when
                            // block_size > hop_size (APO 480/528 > 256), multiple
                            // hops are needed per block. Re-adding the same
                            // windowed frame at each hop position is correct OLA
                            // behavior: the Hann window at 75% overlap sums to
                            // constant gain, and the signal changes negligibly
                            // over one hop (5.3ms at 48kHz). This avoids the
                            // amplitude dips caused by advance_write_only().
                            for _ in 0..ctx.hops_this_block {
                                self.ola_buffer.add_frame(&self.pilot_istft_buf[..fft_size]);
                            }
                        }
                    }
                }

                // Step 2: Drain ALL available hops from OLA into FIFO
                while self.ola_buffer.readable() > 0 {
                    let write_start = self.ola_drain_pos;
                    let write_end = write_start + self.ola_hop_size;
                    if write_end > self.ola_drain.len() {
                        break; // FIFO full
                    }
                    let n = self.ola_buffer.read_hop(
                        &mut self.ola_drain[write_start..write_end],
                    );
                    if n > 0 {
                        self.ola_drain_pos += n;
                    } else {
                        break;
                    }
                }

                // Step 3: Serve block_size samples from FIFO to validated.data
                let serve = block_size
                    .min(self.ola_drain_pos)
                    .min(ctx.validated.data.len());
                if serve > 0 {
                    ctx.validated.data[..serve]
                        .copy_from_slice(&self.ola_drain[..serve]);
                }
                let vd_len = block_size.min(ctx.validated.data.len());
                for i in serve..vd_len {
                    ctx.validated.data[i] = 0.0;
                }

                // Step 4: Compact FIFO (shift remaining data to front)
                let remaining = self.ola_drain_pos - serve;
                if remaining > 0 {
                    self.ola_drain.copy_within(serve..self.ola_drain_pos, 0);
                }
                self.ola_drain_pos = remaining;
            }
            _ => {
                // LegacyAdditive (proven path) or FftOlaFull (future).
                synthesizer::synthesize(
                    ctx.synthesis_mode,
                    &self.combined_buf[..num_synth_bins],
                    &ctx.lattice.core.phase[..num_synth_bins],
                    cutoff_bin,
                    fft_size,
                    scale,
                    &mut ctx.validated.data,
                );
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
        self.pilot_istft_buf.fill(0.0);
        self.ola_buffer.reset();
        self.ola_drain.fill(0.0);
        self.ola_drain_pos = 0; // empty FIFO
    }
}

/// Combine freq-domain residual components into a single magnitude vector.
/// Zero-alloc: writes into pre-allocated `out` buffer.
///
/// Only freq-domain magnitudes are summed here (harmonic + air + phase×0.1).
/// Intentionally excluded:
///   - `side`: ratio-like coefficient (0–ρ_max), not a magnitude — requires
///     cross-channel context to produce actual side-channel energy.
///   - transient/declip: time-domain corrections that flow through the
///     independent `time_candidate` → `time_residual` path with their own gain.
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

    // ── Phase B1-B: Pilot ISTFT + OLA tests ──

    #[test]
    fn pilot_ola_produces_finite_output_on_hop() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 5000.0;
        ctx.damage.overall_confidence = 1.0;
        ctx.hops_this_block = 1; // simulate hop boundary

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        ctx.residual.harmonic[200] = 0.3;

        let mut samples = vec![0.0; 128];
        m5.process(&mut samples, &mut ctx);

        assert!(
            ctx.validated.data.iter().all(|s| s.is_finite()),
            "Pilot OLA output should be finite"
        );
        let energy: f32 = ctx.validated.data.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "Pilot OLA should produce non-zero output on hop");
    }

    #[test]
    fn pilot_ola_outputs_zeros_before_first_hop() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;
        let mut ctx = ProcessContext::new(48000, 2, config);
        ctx.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 5000.0;
        ctx.damage.overall_confidence = 1.0;
        ctx.hops_this_block = 0; // no hop yet

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        ctx.residual.harmonic[200] = 0.3;

        let mut samples = vec![0.0; 128];
        m5.process(&mut samples, &mut ctx);

        // Before first hop, OLA drain is exhausted → zeros
        let max_val = ctx.validated.data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_val < 1e-6, "No hop → zero output, got {max_val}");
    }

    #[test]
    fn pilot_ola_zero_residual_produces_zero() {
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        ctx.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx.lattice = TriLattice::new();
        ctx.damage.cutoff.mean = 15000.0;
        ctx.damage.overall_confidence = 0.8;
        ctx.hops_this_block = 1;

        let core_bins = ctx.lattice.core.num_bins();
        ctx.residual = ResidualCandidate::new(core_bins);
        // All zeros

        let mut samples = vec![0.5; 128];
        m5.process(&mut samples, &mut ctx);

        let max_val = ctx.validated.data.iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(max_val < 1e-6, "Zero residual → zero pilot OLA output, got {max_val}");
    }

    #[test]
    fn pilot_ola_drains_across_blocks() {
        // Simulate 2-block-per-hop scenario (Standard mode: hop=256, block=128)
        let mut m5 = SelfReprojectionValidator::new();
        m5.init(128, 48000);
        let mut config = EngineConfig::default();
        config.strength = 1.0;

        // Block 1: no hop (accumulating)
        let mut ctx1 = ProcessContext::new(48000, 2, config);
        ctx1.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx1.lattice = TriLattice::new();
        ctx1.damage.cutoff.mean = 5000.0;
        ctx1.damage.overall_confidence = 1.0;
        ctx1.hops_this_block = 0;
        let core_bins = ctx1.lattice.core.num_bins();
        ctx1.residual = ResidualCandidate::new(core_bins);
        ctx1.residual.harmonic[200] = 0.3;
        let mut samples1 = vec![0.0; 128];
        m5.process(&mut samples1, &mut ctx1);
        let energy1: f32 = ctx1.validated.data.iter().map(|s| s * s).sum();

        // Block 2: hop boundary crossed
        let mut ctx2 = ProcessContext::new(48000, 2, config);
        ctx2.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx2.lattice = TriLattice::new();
        ctx2.damage.cutoff.mean = 5000.0;
        ctx2.damage.overall_confidence = 1.0;
        ctx2.hops_this_block = 1; // hop!
        ctx2.residual = ResidualCandidate::new(core_bins);
        ctx2.residual.harmonic[200] = 0.3;
        let mut samples2 = vec![0.0; 128];
        m5.process(&mut samples2, &mut ctx2);
        let energy2: f32 = ctx2.validated.data.iter().map(|s| s * s).sum();

        // Block 3: no hop (draining second half of hop output)
        let mut ctx3 = ProcessContext::new(48000, 2, config);
        ctx3.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx3.lattice = TriLattice::new();
        ctx3.damage.cutoff.mean = 5000.0;
        ctx3.damage.overall_confidence = 1.0;
        ctx3.hops_this_block = 0;
        ctx3.residual = ResidualCandidate::new(core_bins);
        ctx3.residual.harmonic[200] = 0.3;
        let mut samples3 = vec![0.0; 128];
        m5.process(&mut samples3, &mut ctx3);
        let energy3: f32 = ctx3.validated.data.iter().map(|s| s * s).sum();

        // Block 1: no hop, no prior data → zeros
        assert!(energy1 < 1e-10, "Block 1 (no hop): should be zero, got {energy1}");
        // Block 2: hop boundary → first half of hop drained
        assert!(energy2 > 0.0, "Block 2 (hop): should have energy, got {energy2}");
        // Block 3: no hop, draining second half
        assert!(energy3 > 0.0, "Block 3 (drain): should have energy, got {energy3}");
    }

    #[test]
    fn pilot_ola_matches_legacy_direction() {
        // Both paths should produce energy for the same bin.
        // They won't be identical (additive vs ISTFT+OLA), but should agree in direction.
        let mut m5_legacy = SelfReprojectionValidator::new();
        let mut m5_pilot = SelfReprojectionValidator::new();
        m5_legacy.init(128, 48000);
        m5_pilot.init(128, 48000);

        let mut config = EngineConfig::default();
        config.strength = 1.0;

        // Legacy run
        let mut ctx_legacy = ProcessContext::new(48000, 2, config);
        ctx_legacy.synthesis_mode = crate::frame::SynthesisMode::LegacyAdditive;
        ctx_legacy.lattice = TriLattice::new();
        ctx_legacy.damage.cutoff.mean = 5000.0;
        ctx_legacy.damage.overall_confidence = 1.0;
        let core_bins = ctx_legacy.lattice.core.num_bins();
        ctx_legacy.residual = ResidualCandidate::new(core_bins);
        ctx_legacy.residual.harmonic[200] = 0.3;
        let mut samples_l = vec![0.0; 128];
        m5_legacy.process(&mut samples_l, &mut ctx_legacy);

        // Pilot OLA run (with hop)
        let mut ctx_pilot = ProcessContext::new(48000, 2, config);
        ctx_pilot.synthesis_mode = crate::frame::SynthesisMode::FftOlaPilot;
        ctx_pilot.lattice = TriLattice::new();
        ctx_pilot.damage.cutoff.mean = 5000.0;
        ctx_pilot.damage.overall_confidence = 1.0;
        ctx_pilot.hops_this_block = 1;
        ctx_pilot.residual = ResidualCandidate::new(core_bins);
        ctx_pilot.residual.harmonic[200] = 0.3;
        let mut samples_p = vec![0.0; 128];
        m5_pilot.process(&mut samples_p, &mut ctx_pilot);

        let legacy_energy: f32 = ctx_legacy.validated.data.iter().map(|s| s * s).sum();
        let pilot_energy: f32 = ctx_pilot.validated.data.iter().map(|s| s * s).sum();

        assert!(legacy_energy > 0.0, "Legacy should produce energy");
        assert!(pilot_energy > 0.0, "Pilot OLA should produce energy on hop");
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
