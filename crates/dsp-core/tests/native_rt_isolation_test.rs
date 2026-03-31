//! Isolation test: pinpoint which native-rt code path causes SNR degradation.
//!
//! Compile with: cargo test --features native-rt -p phaselith-dsp-core --test native_rt_isolation_test
//!
//! Compares _into functions vs allocating versions side-by-side.
//! Also tests effect of pre-allocating ctx.validated.data to 1024 vs 480.

use phaselith_dsp_core::config::EngineConfig;
use phaselith_dsp_core::engine::PhaselithEngineBuilder;
use phaselith_dsp_core::modules::m5_reprojection::{acceptance, constraints, error};

const SR: u32 = 48000;
const BLOCK: usize = 480;
const FFT: usize = 1024;

fn rms(s: &[f32]) -> f32 {
    (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
}

fn snr_db(original: &[f32], modified: &[f32]) -> f32 {
    let n = original.len().min(modified.len());
    let diff: Vec<f32> = (0..n).map(|i| modified[i] - original[i]).collect();
    let diff_rms = rms(&diff);
    let orig_rms = rms(&original[..n]);
    if diff_rms > 1e-10 {
        20.0 * (orig_rms / diff_rms).log10()
    } else {
        f32::INFINITY
    }
}

fn sine(freq: f32, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.7 * (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin())
        .collect()
}

// ─── Test 1: compute_reprojection_error_into vs compute_reprojection_error ───

#[cfg(feature = "native-rt")]
#[test]
fn isolation_error_into_matches_allocating() {
    let original: Vec<f32> = (0..513).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
    let reprojected: Vec<f32> = (0..513).map(|i| (i as f32 * 0.01).sin() * 0.5 + 0.02).collect();

    let alloc_result = error::compute_reprojection_error(&original, &reprojected, 513);

    let mut into_result = vec![0.0f32; 513];
    error::compute_reprojection_error_into(&original, &reprojected, 513, &mut into_result);

    let mut max_diff = 0.0f32;
    for i in 0..513 {
        let d = (alloc_result[i] - into_result[i]).abs();
        max_diff = max_diff.max(d);
    }

    eprintln!("  error _into vs alloc max diff: {:.10}", max_diff);
    assert!(max_diff < 1e-6, "error _into diverges: max_diff={max_diff}");
}

// ─── Test 2: compute_wiener_mask_into vs compute_wiener_mask ───

#[cfg(feature = "native-rt")]
#[test]
fn isolation_wiener_mask_into_matches_with_residual() {
    // Case: residual is NOT empty (Wiener gain path)
    let error_data: Vec<f32> = (0..513).map(|i| 0.001 + (i as f32 * 0.0001)).collect();
    let residual: Vec<f32> = (0..513).map(|i| 0.01 + (i as f32 * 0.001).sin() * 0.05).collect();
    let cutoff_bin = 50;
    let dynamics = 0.6;

    let alloc_mask = acceptance::compute_wiener_mask(&error_data, &residual, cutoff_bin, dynamics);

    let mut into_mask = vec![0.0f32; 513];
    acceptance::compute_wiener_mask_into(&error_data, &residual, cutoff_bin, dynamics, &mut into_mask);

    let mut max_diff = 0.0f32;
    for i in 0..513 {
        let d = (alloc_mask[i] - into_mask[i]).abs();
        max_diff = max_diff.max(d);
    }

    eprintln!("  wiener_mask _into vs alloc (with residual) max diff: {:.10}", max_diff);
    assert!(max_diff < 1e-6, "wiener_mask _into diverges with residual: max_diff={max_diff}");
}

#[cfg(feature = "native-rt")]
#[test]
fn isolation_wiener_mask_into_matches_without_residual() {
    // Case: residual IS empty (fallback threshold path — KNOWN DIFFERENT ALGORITHM)
    let error_data: Vec<f32> = (0..513).map(|i| 0.001 + (i as f32 * 0.0001)).collect();
    let cutoff_bin = 50;
    let dynamics = 0.6;

    let alloc_mask = acceptance::compute_wiener_mask(&error_data, &[], cutoff_bin, dynamics);

    let mut into_mask = vec![0.0f32; 513];
    acceptance::compute_wiener_mask_into(&error_data, &[], cutoff_bin, dynamics, &mut into_mask);

    let mut max_diff = 0.0f32;
    let mut num_diff = 0;
    for i in 0..513 {
        let d = (alloc_mask[i] - into_mask[i]).abs();
        if d > 1e-4 {
            num_diff += 1;
        }
        max_diff = max_diff.max(d);
    }

    eprintln!("  wiener_mask empty-residual max diff: {:.10} ({} bins differ > 1e-4)", max_diff, num_diff);
    // This path is EXPECTED to differ (mean*2 vs median+MAD)
    // But in practice, combined_buf is always passed as non-empty residual
    // Log but don't assert equivalence
}

// ─── Test 3: apply_constraints_styled_into vs apply_constraints_styled ───

#[cfg(feature = "native-rt")]
#[test]
fn isolation_constraints_into_matches_allocating() {
    let mask: Vec<f32> = (0..513).map(|i| (i as f32 / 513.0).min(1.0)).collect();
    let transient_field: Vec<f32> = (0..513).map(|i| if i > 2 && i < 5 { 0.8 } else { 0.0 }).collect();
    let cutoff_bin = 300;

    let alloc_result = constraints::apply_constraints_styled(
        &mask, cutoff_bin, SR, FFT, 0.5, &transient_field, false, 0.0, &[],
    );

    let mut into_result = vec![0.0f32; 513];
    constraints::apply_constraints_styled_into(
        &mask, cutoff_bin, SR, FFT, 0.5, &transient_field, false, 0.0, &[], &mut into_result,
    );

    let mut max_diff = 0.0f32;
    for i in 0..513 {
        let d = (alloc_result[i] - into_result[i]).abs();
        max_diff = max_diff.max(d);
    }

    eprintln!("  constraints _into vs alloc max diff: {:.10}", max_diff);
    assert!(max_diff < 1e-6, "constraints _into diverges: max_diff={max_diff}");
}

// ─── Test 4: Full pipeline comparison — native-rt build vs same build ───
// The KEY test: process the same signal and measure quality.

#[test]
fn isolation_full_pipeline_quality() {
    eprintln!("\n============================================================");
    eprintln!("=== Isolation: Full Pipeline Quality (native-rt build) ===");
    eprintln!("============================================================");

    let total = 200 * BLOCK;
    let input = sine(440.0, total);

    let config = EngineConfig::default();
    let fft_size = config.quality_mode.core_fft_size();
    let mut engine = PhaselithEngineBuilder::new(SR, fft_size)
        .with_config(config)
        .with_channels(1)
        .build_default();

    let num_blocks = input.len() / BLOCK;
    let mut output = Vec::with_capacity(num_blocks * BLOCK);

    for b in 0..num_blocks {
        let start = b * BLOCK;
        let mut block: Vec<f32> = input[start..start + BLOCK].to_vec();
        engine.process(&mut block);
        output.extend_from_slice(&block);
    }

    let snr = snr_db(&input, &output);
    let max_diff = input.iter().zip(output.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);

    eprintln!("  SNR: {:.1} dB", snr);
    eprintln!("  Max diff: {:.6}", max_diff);

    #[cfg(feature = "native-rt")]
    eprintln!("  [compiled WITH native-rt]");
    #[cfg(not(feature = "native-rt"))]
    eprintln!("  [compiled WITHOUT native-rt]");

    assert!(snr > 20.0, "Pipeline SNR too low: {snr:.1} dB");
}

// ─── Test 5: Truly transparent mode (strength=0, no character) ───

#[test]
fn isolation_transparent_mode() {
    eprintln!("\n============================================================");
    eprintln!("=== Isolation: Transparent Mode ===");
    eprintln!("============================================================");

    let total = 100 * BLOCK;
    let input = sine(1000.0, total);

    let mut config = EngineConfig::default();
    config.strength = 0.0;
    config.style = phaselith_dsp_core::config::StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    let fft_size = config.quality_mode.core_fft_size();
    let mut engine = PhaselithEngineBuilder::new(SR, fft_size)
        .with_config(config)
        .with_channels(1)
        .build_default();

    let num_blocks = input.len() / BLOCK;
    let mut output = Vec::with_capacity(num_blocks * BLOCK);

    for b in 0..num_blocks {
        let start = b * BLOCK;
        let mut block: Vec<f32> = input[start..start + BLOCK].to_vec();
        engine.process(&mut block);
        output.extend_from_slice(&block);
    }

    let snr = snr_db(&input, &output);
    let max_diff = input.iter().zip(output.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);

    eprintln!("  SNR: {:.1} dB (should be >50)", snr);
    eprintln!("  Max diff: {:.8}", max_diff);

    #[cfg(feature = "native-rt")]
    eprintln!("  [compiled WITH native-rt]");
    #[cfg(not(feature = "native-rt"))]
    eprintln!("  [compiled WITHOUT native-rt]");

    assert!(snr > 50.0, "Transparent mode SNR too low: {snr:.1} dB — native-rt path leaks signal modification");
}

// ─── Test 6: Per-module SNR in transparent mode ───
// Identify exactly which module introduces degradation.

#[test]
fn isolation_per_module_transparent() {
    use phaselith_dsp_core::module_trait::{PhaselithModule, ProcessContext};
    use phaselith_dsp_core::fft::planner::SharedFftPlans;
    use phaselith_dsp_core::types::CORE_FFT_SIZE;

    eprintln!("\n============================================================");
    eprintln!("=== Isolation: Per-Module SNR (Transparent Mode) ===");
    eprintln!("============================================================");

    let mut config = EngineConfig::default();
    config.strength = 0.0;
    config.style = phaselith_dsp_core::config::StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    let mut plans = SharedFftPlans::new();
    let mut m0 = phaselith_dsp_core::modules::m0_orchestrator::FrameOrchestrator::new();
    m0.init(FFT, SR);
    let mut m1 = phaselith_dsp_core::modules::m1_damage::DamagePosteriorEngine::new();
    m1.init(FFT, SR);
    let mut m2 = phaselith_dsp_core::modules::m2_lattice::TriLatticeAnalysis::new();
    m2.init_with_plans(SR, &mut plans);
    let mut m3 = phaselith_dsp_core::modules::m3_factorizer::StructuredFactorizer::new();
    m3.init(FFT, SR);
    let mut m4 = phaselith_dsp_core::modules::m4_solver::InverseResidualSolver::new();
    m4.init(FFT, SR);
    let mut m5 = phaselith_dsp_core::modules::m5_reprojection::SelfReprojectionValidator::new();
    m5.init_with_plans(FFT, SR, &mut plans);
    let mut m6 = phaselith_dsp_core::modules::m6_mixer::PerceptualSafetyMixer::new();
    m6.init(FFT, SR);
    let mut m7 = phaselith_dsp_core::modules::m7_governor::QualityGovernor::new();
    m7.init(FFT, SR);

    let mut ctx = ProcessContext::new(SR, 1, config);
    ctx.frame_params = phaselith_dsp_core::frame::FrameParams::new(FFT, SR, config.quality_mode);
    ctx.dry_buffer = vec![0.0; FFT];

    #[cfg(feature = "native-rt")]
    {
        use phaselith_dsp_core::types::{StructuredFields, ResidualCandidate, ValidatedResidual};
        let core_bins = CORE_FFT_SIZE / 2 + 1;
        ctx.fields = StructuredFields::new(core_bins);
        ctx.residual = ResidualCandidate::new(core_bins);
        ctx.validated = ValidatedResidual::new(FFT);
        ctx.validated.acceptance_mask = vec![1.0; core_bins];
        ctx.time_candidate = vec![0.0; FFT];
    }

    let input = sine(440.0, 50 * BLOCK);

    // Process block 40 (well past warmup, after M1 update at frame 32)
    // First warm up: process blocks 0-39
    for b in 0..40 {
        let start = b * BLOCK;
        let mut block: Vec<f32> = input[start..start + BLOCK].to_vec();
        let dry_len = block.len().min(ctx.dry_buffer.len());
        ctx.dry_buffer[..dry_len].copy_from_slice(&block[..dry_len]);
        ctx.frame_index += 1;
        m0.process(&mut block, &mut ctx);
        m1.process(&mut block, &mut ctx);
        m2.process(&mut block, &mut ctx);
        m3.process(&mut block, &mut ctx);
        m4.process(&mut block, &mut ctx);
        m5.process(&mut block, &mut ctx);
        m6.process(&mut block, &mut ctx);
        m7.process(&mut block, &mut ctx);
    }

    // Now process block 40 step by step, measuring SNR after each module
    let b = 40;
    let start = b * BLOCK;
    let original: Vec<f32> = input[start..start + BLOCK].to_vec();
    let mut block = original.clone();
    let dry_len = block.len().min(ctx.dry_buffer.len());
    ctx.dry_buffer[..dry_len].copy_from_slice(&block[..dry_len]);
    ctx.frame_index += 1;

    m0.process(&mut block, &mut ctx);
    eprintln!("  After M0: SNR={:.1}dB max_diff={:.8}", snr_db(&original, &block), max_abs_diff(&original, &block));

    m1.process(&mut block, &mut ctx);
    eprintln!("  After M1: SNR={:.1}dB max_diff={:.8}", snr_db(&original, &block), max_abs_diff(&original, &block));

    m2.process(&mut block, &mut ctx);
    eprintln!("  After M2: SNR={:.1}dB max_diff={:.8}", snr_db(&original, &block), max_abs_diff(&original, &block));

    m3.process(&mut block, &mut ctx);
    let peak_t = ctx.fields.transient.iter().copied().fold(0.0f32, f32::max);
    let pre_echo_amount = (ctx.config.transient * ctx.config.pre_echo_transient_scaling).clamp(0.0, 1.0);
    let t_activity = peak_t.max(ctx.fields.spectral_flux).clamp(0.0, 1.0);
    eprintln!("  After M3: SNR={:.1}dB max_diff={:.8} flux={:.6} peak_t={:.6} pre_echo_amt={:.2} t_act={:.6} hops={}",
        snr_db(&original, &block), max_abs_diff(&original, &block),
        ctx.fields.spectral_flux, peak_t, pre_echo_amount, t_activity, ctx.hops_this_block);

    m4.process(&mut block, &mut ctx);
    eprintln!("  After M4: SNR={:.1}dB max_diff={:.8}", snr_db(&original, &block), max_abs_diff(&original, &block));

    m5.process(&mut block, &mut ctx);
    eprintln!("  After M5: SNR={:.1}dB max_diff={:.8} val_max={:.8}",
        snr_db(&original, &block), max_abs_diff(&original, &block),
        ctx.validated.data.iter().take(BLOCK).map(|x| x.abs()).fold(0.0f32, f32::max));

    m6.process(&mut block, &mut ctx);
    eprintln!("  After M6: SNR={:.1}dB max_diff={:.8}", snr_db(&original, &block), max_abs_diff(&original, &block));

    m7.process(&mut block, &mut ctx);
    eprintln!("  After M7: SNR={:.1}dB max_diff={:.8}", snr_db(&original, &block), max_abs_diff(&original, &block));

    let final_snr = snr_db(&original, &block);
    eprintln!("  Config: strength={} warmth={} smooth={} transient={} enabled={}",
        ctx.config.strength, ctx.config.style.warmth, ctx.config.style.smoothness,
        ctx.config.transient, ctx.config.enabled);

    assert!(final_snr > 50.0, "Transparent mode per-module SNR too low: {final_snr:.1} dB");
}

fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
}

// ─── Test 7: Module-by-module isolation (default config) ───
// Run each module individually and compare output.

#[test]
fn isolation_module_by_module() {
    use phaselith_dsp_core::module_trait::{PhaselithModule, ProcessContext};
    use phaselith_dsp_core::fft::planner::SharedFftPlans;
    use phaselith_dsp_core::types::{TriLattice, CORE_FFT_SIZE};

    eprintln!("\n============================================================");
    eprintln!("=== Isolation: Module-by-Module ===");
    eprintln!("============================================================");

    let config = EngineConfig::default();

    // Create shared plans
    let mut plans = SharedFftPlans::new();

    // Initialize M2
    let mut m2 = phaselith_dsp_core::modules::m2_lattice::TriLatticeAnalysis::new();
    m2.init_with_plans(SR, &mut plans);

    // Initialize M1
    let mut m1 = phaselith_dsp_core::modules::m1_damage::DamagePosteriorEngine::new();
    m1.init(FFT, SR);

    // Initialize M3
    let mut m3 = phaselith_dsp_core::modules::m3_factorizer::StructuredFactorizer::new();
    m3.init(FFT, SR);

    // Initialize M4
    let mut m4 = phaselith_dsp_core::modules::m4_solver::InverseResidualSolver::new();
    m4.init(FFT, SR);

    // Initialize M5
    let mut m5 = phaselith_dsp_core::modules::m5_reprojection::SelfReprojectionValidator::new();
    m5.init_with_plans(FFT, SR, &mut plans);

    // Initialize M6
    let mut m6 = phaselith_dsp_core::modules::m6_mixer::PerceptualSafetyMixer::new();
    m6.init(FFT, SR);

    // Initialize M7
    let mut m7 = phaselith_dsp_core::modules::m7_governor::QualityGovernor::new();
    m7.init(FFT, SR);

    // Initialize M0
    let mut m0 = phaselith_dsp_core::modules::m0_orchestrator::FrameOrchestrator::new();
    m0.init(FFT, SR);

    // Create context — manually mimicking what engine builder does
    let mut ctx = ProcessContext::new(SR, 1, config);
    ctx.frame_params = phaselith_dsp_core::frame::FrameParams::new(FFT, SR, config.quality_mode);
    ctx.dry_buffer = vec![0.0; FFT];

    // Pre-allocate like native-rt engine builder does
    #[cfg(feature = "native-rt")]
    {
        use phaselith_dsp_core::types::{StructuredFields, ResidualCandidate, ValidatedResidual};
        let core_bins = CORE_FFT_SIZE / 2 + 1;
        ctx.fields = StructuredFields::new(core_bins);
        ctx.residual = ResidualCandidate::new(core_bins);
        ctx.validated = ValidatedResidual::new(FFT); // 1024!
        ctx.validated.acceptance_mask = vec![1.0; core_bins];
        ctx.time_candidate = vec![0.0; FFT];
        eprintln!("  ctx.validated.data.len() = {} (pre-allocated)", ctx.validated.data.len());
    }
    #[cfg(not(feature = "native-rt"))]
    {
        eprintln!("  ctx.validated.data.len() = {} (will be allocated by M5)", ctx.validated.data.len());
    }

    // Process 100 blocks through M0→M1→M2→M3→M4→M5→M6→M7
    let input = sine(440.0, 100 * BLOCK);
    let mut output = Vec::with_capacity(100 * BLOCK);

    for b in 0..100 {
        let start = b * BLOCK;
        let mut block: Vec<f32> = input[start..start + BLOCK].to_vec();

        // Save dry
        let dry_len = block.len().min(ctx.dry_buffer.len());
        ctx.dry_buffer[..dry_len].copy_from_slice(&block[..dry_len]);
        ctx.frame_index += 1;

        m0.process(&mut block, &mut ctx);
        m1.process(&mut block, &mut ctx);
        m2.process(&mut block, &mut ctx);
        m3.process(&mut block, &mut ctx);
        m4.process(&mut block, &mut ctx);

        // Capture state BEFORE M5
        let residual_energy: f32 = ctx.residual.harmonic.iter().map(|x| x * x).sum();
        let cutoff = ctx.damage.cutoff.mean;

        m5.process(&mut block, &mut ctx);

        // Capture M5 output
        let validated_energy: f32 = ctx.validated.data.iter().take(BLOCK).map(|x| x * x).sum();
        let validated_max: f32 = ctx.validated.data.iter().take(BLOCK).map(|x| x.abs()).fold(0.0f32, f32::max);

        m6.process(&mut block, &mut ctx);
        m7.process(&mut block, &mut ctx);

        output.extend_from_slice(&block);

        // Log key metrics for first few and then periodically
        if b < 5 || b == 32 || b == 50 || b == 99 {
            let block_snr = snr_db(&input[start..start + BLOCK], &block);
            eprintln!(
                "  Block {:3}: SNR={:6.1}dB cutoff={:.0}Hz res_E={:.6} val_E={:.6} val_max={:.6} conf={:.3}",
                b, block_snr, cutoff, residual_energy, validated_energy, validated_max,
                ctx.damage.overall_confidence
            );
        }
    }

    let overall_snr = snr_db(&input, &output);
    eprintln!("\n  Overall SNR: {:.1} dB", overall_snr);
    assert!(overall_snr > 20.0, "Module-by-module SNR too low: {overall_snr:.1} dB");
}

// ─── Test 8: StftEngine vs analyze_lattice energy comparison for zero-padded input ───
// This is the smoking gun test: compare energy values between both paths
// for the exact APO scenario (480 samples zero-padded to 1024/256).

#[test]
fn isolation_stft_engine_vs_legacy_energy_zero_padded() {
    use phaselith_dsp_core::modules::m2_lattice::stft;
    use phaselith_dsp_core::types::{Lattice, MICRO_FFT_SIZE, CORE_FFT_SIZE};
    use phaselith_dsp_core::modules::m3_factorizer::transient;

    eprintln!("\n============================================================");
    eprintln!("=== StftEngine vs analyze_lattice: zero-padded 480 samples ===");
    eprintln!("============================================================");

    // Generate 480-sample block of 440 Hz sine (the APO case)
    let samples_480 = sine(440.0, BLOCK);

    // ── MICRO lattice (256-FFT) ──
    // Both paths: 256 samples from the 480-sample block (fully filled, no zero padding)
    let mut micro_scratch = vec![0.0f32; MICRO_FFT_SIZE];
    let micro_len = BLOCK.min(MICRO_FFT_SIZE); // 256
    micro_scratch[..micro_len].copy_from_slice(&samples_480[..micro_len]);
    // No zero-padding needed: 256 <= 256

    // Legacy path
    let mut micro_legacy = Lattice::new(MICRO_FFT_SIZE);
    stft::analyze_lattice(&micro_scratch[..MICRO_FFT_SIZE], &mut micro_legacy, SR);

    // StftEngine path
    let mut micro_engine_inst = stft::StftEngine::new(MICRO_FFT_SIZE);
    let mut micro_engine_lat = Lattice::new(MICRO_FFT_SIZE);
    micro_engine_inst.analyze(&micro_scratch[..MICRO_FFT_SIZE], &mut micro_engine_lat);

    // Compare micro energy
    let micro_bins = MICRO_FFT_SIZE / 2 + 1;
    let mut micro_max_diff = 0.0f32;
    let mut micro_diff_bins = 0;
    for i in 0..micro_bins {
        let d = (micro_engine_lat.energy[i] - micro_legacy.energy[i]).abs();
        if d > 1e-10 {
            micro_diff_bins += 1;
        }
        micro_max_diff = micro_max_diff.max(d);
    }
    eprintln!("  MICRO (256-FFT): max_energy_diff={:.2e} diff_bins={}/{}", micro_max_diff, micro_diff_bins, micro_bins);

    // ── CORE lattice (1024-FFT) ──
    // Both paths: 480 samples + 544 zeros
    let mut core_scratch = vec![0.0f32; CORE_FFT_SIZE];
    let core_len = BLOCK.min(CORE_FFT_SIZE); // 480
    core_scratch[..core_len].copy_from_slice(&samples_480[..core_len]);
    // Zero-padded: core_scratch[480..1024] = 0.0

    // Legacy path
    let mut core_legacy = Lattice::new(CORE_FFT_SIZE);
    stft::analyze_lattice(&core_scratch[..CORE_FFT_SIZE], &mut core_legacy, SR);

    // StftEngine path
    let mut core_engine_inst = stft::StftEngine::new(CORE_FFT_SIZE);
    let mut core_engine_lat = Lattice::new(CORE_FFT_SIZE);
    core_engine_inst.analyze(&core_scratch[..CORE_FFT_SIZE], &mut core_engine_lat);

    // Compare core energy
    let core_bins = CORE_FFT_SIZE / 2 + 1;
    let mut core_max_diff = 0.0f32;
    let mut core_diff_bins = 0;
    for i in 0..core_bins {
        let d = (core_engine_lat.energy[i] - core_legacy.energy[i]).abs();
        if d > 1e-10 {
            core_diff_bins += 1;
        }
        core_max_diff = core_max_diff.max(d);
    }
    eprintln!("  CORE (1024-FFT): max_energy_diff={:.2e} diff_bins={}/{}", core_max_diff, core_diff_bins, core_bins);

    // ── Check total energy ratio (Parseval's gate) ──
    let total_micro: f32 = micro_legacy.energy.iter().sum();
    let total_core: f32 = core_legacy.energy.iter().sum();
    let energy_ratio = if total_core > 1e-10 { total_micro / total_core } else { 0.0 };
    eprintln!("  Total energy: micro={:.6e} core={:.6e} ratio={:.4}", total_micro, total_core, energy_ratio);

    // ── Run detect_transients with BOTH sets of energy ──
    let mut transient_legacy = vec![0.0f32; core_bins];
    transient::detect_transients(&micro_legacy.energy, &core_legacy.energy, &mut transient_legacy);
    let peak_t_legacy = transient_legacy.iter().copied().fold(0.0f32, f32::max);

    let mut transient_engine = vec![0.0f32; core_bins];
    transient::detect_transients(&micro_engine_lat.energy, &core_engine_lat.energy, &mut transient_engine);
    let peak_t_engine = transient_engine.iter().copied().fold(0.0f32, f32::max);

    eprintln!("  detect_transients peak_t: legacy={:.6} engine={:.6}", peak_t_legacy, peak_t_engine);

    // Dump bins where transient_field > 0.5 to understand the pattern
    for k in 0..core_bins {
        if transient_legacy[k] > 0.1 || transient_engine[k] > 0.1 {
            let micro_k = (k * micro_bins) / core_bins.max(1);
            eprintln!("    bin {:3}: legacy_t={:.4} engine_t={:.4} | micro_k={} micro_e_leg={:.2e} micro_e_eng={:.2e} core_e_leg={:.2e} core_e_eng={:.2e}",
                k, transient_legacy[k], transient_engine[k],
                micro_k,
                if micro_k < micro_bins { micro_legacy.energy[micro_k] } else { -1.0 },
                if micro_k < micro_bins { micro_engine_lat.energy[micro_k] } else { -1.0 },
                core_legacy.energy[k],
                core_engine_lat.energy[k],
            );
        }
    }

    // Both paths should produce identical results
    assert!(
        (peak_t_legacy - peak_t_engine).abs() < 0.01,
        "StftEngine and analyze_lattice produce different transient results: legacy={peak_t_legacy} engine={peak_t_engine}"
    );
}
