//! APO Instance Switch Pop Detection Test
//!
//! Simulates the exact Windows APO lifecycle during audio graph reconfiguration:
//! - UAC prompts, YY Voice channel join/leave, etc.
//! - Windows destroys old APO instance, creates new one
//! - New instance may get all-zero or partially-zero first input blocks
//!
//! Two pop patterns tested:
//! Pattern 1 (PARTIAL_ZERO): reused engine, first block has leading zeros then
//!   real audio. Engine outputs near-zero for zero portion → boundary discontinuity.
//! Pattern 2 (ALL_ZERO): first block is entirely zero. Must not hard-cut from
//!   previous output level to silence.
//!
//! Both patterns must produce output with no boundary discontinuity > threshold.

use phaselith_dsp_core::config::{EngineConfig, PhaseMode, QualityMode, SynthesisMode};
use phaselith_dsp_core::engine::{PhaselithEngine, PhaselithEngineBuilder};
use phaselith_dsp_core::types::CrossChannelContext;

const SAMPLE_RATE: u32 = 48000;
/// Real APO uses 528 (11ms), matching Windows audio engine default.
const APO_BLOCK_SIZE: usize = 528;
/// Number of blocks to run before simulating instance switch.
const WARMUP_BLOCKS: usize = 100;
/// Number of blocks to prime fresh engines with.
const PRIME_FRAMES: usize = 32;
/// Max acceptable boundary discontinuity (sample-to-sample jump at block edge).
/// 0.01 ≈ -40dB, well below audible click threshold.
const MAX_BOUNDARY_DISC: f32 = 0.02;

/// APO-matching config (LegacyAdditive, Standard quality, default strength).
fn apo_config() -> EngineConfig {
    EngineConfig {
        enabled: true,
        strength: 0.5,
        hf_reconstruction: 0.5,
        dynamics: 0.5,
        transient: 0.5,
        pre_echo_transient_scaling: 0.4,
        declip_transient_scaling: 1.0,
        delayed_transient_repair: false,
        phase_mode: PhaseMode::Linear,
        quality_mode: QualityMode::Standard,
        synthesis_mode: SynthesisMode::LegacyAdditive,
        ambience_preserve: 0.0,
        ..EngineConfig::default()
    }
}

/// Generate continuous sine wave.
fn sine_wave(freq: f32, num_samples: usize) -> Vec<f32> {
    (0..num_samples)
        .map(|i| {
            0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE as f32).sin()
        })
        .collect()
}

/// Generate band-limited signal (simulates real music).
fn bandlimited_signal(num_samples: usize) -> Vec<f32> {
    let cutoff_hz = 14000.0f32;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
    let dt = 1.0 / SAMPLE_RATE as f32;
    let alpha = dt / (rc + dt);

    let mut output = Vec::with_capacity(num_samples);
    let mut prev = 0.0f32;
    for i in 0..num_samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let mut s = 0.0f32;
        for k in (1..=15).step_by(2) {
            let freq = 440.0 * k as f32;
            if freq < SAMPLE_RATE as f32 / 2.0 {
                s += (2.0 * std::f32::consts::PI * freq * t).sin() / k as f32;
            }
        }
        s *= 0.4;
        prev = prev + alpha * (s - prev);
        output.push(prev);
    }
    output
}

/// Simulated APO instance with dual engines, mimicking PhaselithApo lifecycle.
struct SimulatedApo {
    engine_l: PhaselithEngine,
    engine_r: PhaselithEngine,
    cross_channel_prev: Option<CrossChannelContext>,
    last_output_l: f32,
    last_output_r: f32,
    frame_count: u64,
    /// Number of consecutive zero-input blocks that were faded out.
    /// When > 0 and real audio resumes, crossfade from 0 into engine output.
    zero_blocks_count: u32,
}

impl SimulatedApo {
    fn new(config: EngineConfig) -> Self {
        Self {
            engine_l: PhaselithEngineBuilder::new(SAMPLE_RATE, APO_BLOCK_SIZE)
                .with_config(config)
                .with_channels(1)
                .build_default(),
            engine_r: PhaselithEngineBuilder::new(SAMPLE_RATE, APO_BLOCK_SIZE)
                .with_config(config)
                .with_channels(1)
                .build_default(),
            cross_channel_prev: None,
            last_output_l: 0.0,
            last_output_r: 0.0,
            frame_count: 0,
            zero_blocks_count: 0,
        }
    }

    /// Number of startup frames where zero-input protection is active.
    const STARTUP_GUARD_FRAMES: u64 = 8;

    /// Check if input block is all zeros.
    fn is_all_zero(buf: &[f32]) -> bool {
        buf.iter().all(|&v| v == 0.0)
    }

    /// Find index of first non-zero sample in buffer.
    fn first_nonzero_index(buf: &[f32]) -> Option<usize> {
        buf.iter().position(|&v| v != 0.0)
    }

    /// Process one block through dual engines with APO-layer protection.
    /// Handles zero and partial-zero input blocks during instance switch.
    fn process_block(&mut self, input_l: &[f32], input_r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let frames = input_l.len();
        self.frame_count += 1;

        // ═══ Zero-input protection (first few frames after instance switch) ═══
        // Windows sends all-zero or partial-zero blocks during audio graph
        // reconfiguration. Don't feed zeros to engine (dilutes OLA state).
        if self.frame_count <= Self::STARTUP_GUARD_FRAMES {
            let all_zero_l = Self::is_all_zero(input_l);
            let all_zero_r = Self::is_all_zero(input_r);

            if all_zero_l && all_zero_r {
                // All-zero block: output smooth fadeout from last_output → 0
                let mut out_l = vec![0.0f32; frames];
                let mut out_r = vec![0.0f32; frames];
                if self.last_output_l.abs() > 1e-6 || self.last_output_r.abs() > 1e-6 {
                    for f in 0..frames {
                        let t = 1.0 - ((f as f32 + 1.0) / frames as f32);
                        let smooth = t * t * (3.0 - 2.0 * t);
                        out_l[f] = self.last_output_l * smooth;
                        out_r[f] = self.last_output_r * smooth;
                    }
                }
                self.last_output_l = 0.0;
                self.last_output_r = 0.0;
                self.zero_blocks_count += 1;
                return (out_l, out_r);
            }

            // Partial-zero block: leading zeros then real audio
            let nz_l = Self::first_nonzero_index(input_l).unwrap_or(0);
            let nz_r = Self::first_nonzero_index(input_r).unwrap_or(0);
            let nz_start = nz_l.min(nz_r);

            if nz_start > 0 {
                // Process only the non-zero portion through engines
                let mut buf_l = input_l.to_vec();
                let mut buf_r = input_r.to_vec();

                // Fill leading zeros with smooth continuation from last_output
                // before engine processing, so engine sees continuous signal
                for f in 0..nz_start {
                    let t = f as f32 / nz_start as f32;
                    let smooth = t * t * (3.0 - 2.0 * t);
                    // Blend from last_output toward the first real sample
                    buf_l[f] = self.last_output_l * (1.0 - smooth) + input_l[nz_start] * smooth;
                    buf_r[f] = self.last_output_r * (1.0 - smooth) + input_r[nz_start] * smooth;
                }

                let dry_l = buf_l.clone();
                let dry_r = buf_r.clone();

                self.engine_l.context_mut().cross_channel = self.cross_channel_prev;
                self.engine_l.process(&mut buf_l);

                self.engine_r.context_mut().cross_channel = self.cross_channel_prev;
                self.engine_r.process(&mut buf_r);

                self.cross_channel_prev =
                    Some(CrossChannelContext::from_lr(&dry_l, &dry_r));

                // Crossfade output over ENTIRE block from last_output to engine output.
                // Not just the leading-zero region — when nz_start is small (e.g., 1),
                // crossfading only 1 sample creates an audible click.
                for f in 0..frames {
                    let t = (f as f32 + 1.0) / frames as f32;
                    let smooth = t * t * (3.0 - 2.0 * t);
                    buf_l[f] = self.last_output_l * (1.0 - smooth) + buf_l[f] * smooth;
                    buf_r[f] = self.last_output_r * (1.0 - smooth) + buf_r[f] * smooth;
                }

                if frames > 0 {
                    self.last_output_l = buf_l[frames - 1];
                    self.last_output_r = buf_r[frames - 1];
                }
                return (buf_l, buf_r);
            }
        }

        // ═══ Normal processing ═══
        let mut buf_l = input_l.to_vec();
        let mut buf_r = input_r.to_vec();
        let dry_l = buf_l.clone();
        let dry_r = buf_r.clone();

        self.engine_l.context_mut().cross_channel = self.cross_channel_prev;
        self.engine_l.process(&mut buf_l);

        self.engine_r.context_mut().cross_channel = self.cross_channel_prev;
        self.engine_r.process(&mut buf_r);

        self.cross_channel_prev = Some(CrossChannelContext::from_lr(&dry_l, &dry_r));

        // Post-zero crossfade: after zero blocks, smoothly fade in engine output
        // from last_output (which is 0 after fadeout). This prevents the
        // discontinuity when transitioning from silence back to real audio.
        if self.zero_blocks_count > 0 {
            self.zero_blocks_count = 0;
            let saved_l = self.last_output_l;
            let saved_r = self.last_output_r;
            for f in 0..frames {
                let t = (f as f32 + 1.0) / frames as f32;
                let smooth = t * t * (3.0 - 2.0 * t);
                buf_l[f] = saved_l * (1.0 - smooth) + buf_l[f] * smooth;
                buf_r[f] = saved_r * (1.0 - smooth) + buf_r[f] * smooth;
            }
        }

        if frames > 0 {
            self.last_output_l = buf_l[frames - 1];
            self.last_output_r = buf_r[frames - 1];
        }

        (buf_l, buf_r)
    }

    /// Prime engines with audio blocks (simulates PRIME_FRAMES in lock_for_process).
    fn prime_with(&mut self, blocks_l: &[Vec<f32>], blocks_r: &[Vec<f32>]) {
        for i in 0..blocks_l.len().min(blocks_r.len()) {
            let mut bl = blocks_l[i].clone();
            let mut br = blocks_r[i].clone();
            self.engine_l.process(&mut bl);
            self.engine_r.process(&mut br);
        }
    }
}

/// Measure boundary discontinuity between last sample of prev block and first
/// sample of next block. Returns max(|jump_l|, |jump_r|).
fn boundary_disc(
    prev_last_l: f32,
    prev_last_r: f32,
    next_first_l: f32,
    next_first_r: f32,
) -> f32 {
    (next_first_l - prev_last_l)
        .abs()
        .max((next_first_r - prev_last_r).abs())
}

/// Simulate the full APO instance switch scenario.
/// 1. Run old instance for WARMUP_BLOCKS on real audio
/// 2. Save engine state + recent input blocks (like STORED_ENGINES + STORED_PRIME)
/// 3. Create new instance, either reuse engines or prime fresh
/// 4. Feed zero/partial-zero first blocks (like Windows does)
/// 5. Resume with real audio
/// 6. Check all boundary discontinuities
///
/// Returns (disc_at_switch, disc_at_resume, all output for analysis).
fn run_instance_switch_scenario(
    signal_l: &[f32],
    signal_r: &[f32],
    switch_type: SwitchType,
) -> (f32, f32, Vec<f32>, Vec<f32>) {
    let config = apo_config();
    let mut apo = SimulatedApo::new(config);

    // Phase 1: Warm up the old instance with real audio
    let mut all_output_l = Vec::new();
    let mut all_output_r = Vec::new();
    let mut recent_blocks_l: Vec<Vec<f32>> = Vec::new();
    let mut recent_blocks_r: Vec<Vec<f32>> = Vec::new();

    for b in 0..WARMUP_BLOCKS {
        let start = b * APO_BLOCK_SIZE;
        let end = start + APO_BLOCK_SIZE;
        if end > signal_l.len() {
            break;
        }
        let (out_l, out_r) = apo.process_block(&signal_l[start..end], &signal_r[start..end]);
        all_output_l.extend_from_slice(&out_l);
        all_output_r.extend_from_slice(&out_r);

        // Store recent blocks for priming (ring buffer of PRIME_FRAMES)
        recent_blocks_l.push(signal_l[start..end].to_vec());
        recent_blocks_r.push(signal_r[start..end].to_vec());
        if recent_blocks_l.len() > PRIME_FRAMES {
            recent_blocks_l.remove(0);
            recent_blocks_r.remove(0);
        }
    }

    let last_l = apo.last_output_l;
    let last_r = apo.last_output_r;
    eprintln!(
        "  Old instance last output: ({:.6}, {:.6})",
        last_l, last_r
    );

    // Phase 2: Simulate instance switch
    // Save old engines (reuse case) or create fresh + prime
    let mut new_apo = match switch_type {
        SwitchType::Reused => {
            // Transfer engines directly (like STORED_ENGINES reuse)
            let mut new = SimulatedApo {
                engine_l: apo.engine_l,
                engine_r: apo.engine_r,
                cross_channel_prev: apo.cross_channel_prev,
                last_output_l: last_l, // Seeded from global atomic
                last_output_r: last_r,
                frame_count: 0,
                zero_blocks_count: 0,
            };
            new
        }
        SwitchType::FreshPrimed => {
            // Fresh engines primed with stored audio (concurrent instance case)
            let mut new = SimulatedApo::new(config);
            new.prime_with(&recent_blocks_l, &recent_blocks_r);
            new.last_output_l = last_l; // Seeded from global atomic
            new.last_output_r = last_r;
            new
        }
    };

    // Phase 3: Feed zero/partial-zero blocks (Windows behavior)
    // First block: all zeros (Windows sends empty block during graph switch)
    let zero_block = vec![0.0f32; APO_BLOCK_SIZE];
    let (out_l, out_r) = new_apo.process_block(&zero_block, &zero_block);

    // *** THIS IS THE KEY CHECK ***
    // The boundary between old instance's last output and new instance's first output
    let disc_at_switch = boundary_disc(last_l, last_r, out_l[0], out_r[0]);
    eprintln!(
        "  Switch boundary disc: {:.6} (first output: ({:.6}, {:.6}))",
        disc_at_switch, out_l[0], out_r[0]
    );

    all_output_l.extend_from_slice(&out_l);
    all_output_r.extend_from_slice(&out_r);

    // Second block may also be zero
    let (out_l, out_r) = new_apo.process_block(&zero_block, &zero_block);
    all_output_l.extend_from_slice(&out_l);
    all_output_r.extend_from_slice(&out_r);

    // Phase 4: Resume with real audio
    let resume_start = (WARMUP_BLOCKS + 2) * APO_BLOCK_SIZE;
    for b in 0..50 {
        let start = resume_start + b * APO_BLOCK_SIZE;
        let end = start + APO_BLOCK_SIZE;
        if end > signal_l.len() {
            break;
        }
        let (out_l, out_r) =
            new_apo.process_block(&signal_l[start..end], &signal_r[start..end]);
        all_output_l.extend_from_slice(&out_l);
        all_output_r.extend_from_slice(&out_r);
    }

    // Also check disc when resuming real audio after zeros
    let zero_end = (WARMUP_BLOCKS + 2) * APO_BLOCK_SIZE;
    let resume_disc = if all_output_l.len() > zero_end {
        let rd = boundary_disc(
            all_output_l[zero_end - 1],
            all_output_r[zero_end - 1],
            all_output_l[zero_end],
            all_output_r[zero_end],
        );
        eprintln!("  Resume boundary disc: {:.6}", rd);
        rd
    } else {
        0.0
    };

    (disc_at_switch, resume_disc, all_output_l, all_output_r)
}

/// Also test partial-zero block (leading zeros then real audio mid-block).
fn run_partial_zero_scenario(
    signal_l: &[f32],
    signal_r: &[f32],
    switch_type: SwitchType,
) -> f32 {
    let config = apo_config();
    let mut apo = SimulatedApo::new(config);

    // Warm up
    let mut recent_blocks_l: Vec<Vec<f32>> = Vec::new();
    let mut recent_blocks_r: Vec<Vec<f32>> = Vec::new();

    for b in 0..WARMUP_BLOCKS {
        let start = b * APO_BLOCK_SIZE;
        let end = start + APO_BLOCK_SIZE;
        if end > signal_l.len() {
            break;
        }
        apo.process_block(&signal_l[start..end], &signal_r[start..end]);
        recent_blocks_l.push(signal_l[start..end].to_vec());
        recent_blocks_r.push(signal_r[start..end].to_vec());
        if recent_blocks_l.len() > PRIME_FRAMES {
            recent_blocks_l.remove(0);
            recent_blocks_r.remove(0);
        }
    }

    let last_l = apo.last_output_l;
    let last_r = apo.last_output_r;

    // Create new instance
    let mut new_apo = match switch_type {
        SwitchType::Reused => SimulatedApo {
            engine_l: apo.engine_l,
            engine_r: apo.engine_r,
            cross_channel_prev: apo.cross_channel_prev,
            last_output_l: last_l,
            last_output_r: last_r,
            frame_count: 0,
            zero_blocks_count: 0,
        },
        SwitchType::FreshPrimed => {
            let mut new = SimulatedApo::new(config);
            new.prime_with(&recent_blocks_l, &recent_blocks_r);
            new.last_output_l = last_l;
            new.last_output_r = last_r;
            new
        }
    };

    // Partial-zero block: first half zeros, second half real audio
    let resume_start = WARMUP_BLOCKS * APO_BLOCK_SIZE;
    let mut partial_l = vec![0.0f32; APO_BLOCK_SIZE];
    let mut partial_r = vec![0.0f32; APO_BLOCK_SIZE];
    let half = APO_BLOCK_SIZE / 2;
    if resume_start + APO_BLOCK_SIZE <= signal_l.len() {
        partial_l[half..].copy_from_slice(&signal_l[resume_start + half..resume_start + APO_BLOCK_SIZE]);
        partial_r[half..].copy_from_slice(&signal_r[resume_start + half..resume_start + APO_BLOCK_SIZE]);
    }

    let (out_l, out_r) = new_apo.process_block(&partial_l, &partial_r);

    let disc = boundary_disc(last_l, last_r, out_l[0], out_r[0]);
    eprintln!(
        "  Partial-zero switch disc: {:.6} (last=({:.6},{:.6}), first=({:.6},{:.6}))",
        disc, last_l, last_r, out_l[0], out_r[0]
    );
    disc
}

#[derive(Clone, Copy)]
enum SwitchType {
    /// Engine state transferred from old instance (sequential switch)
    Reused,
    /// Fresh engine primed with stored audio (concurrent switch)
    FreshPrimed,
}

// ═══════════════════════════════════════════════════════════════════
// Pattern 1: Reused engine + all-zero first block
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pattern1_reused_allzero_sine() {
    eprintln!("\n=== Pattern 1: Reused engine, all-zero block, sine input ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = sine_wave(440.0, total);
    let (disc, resume_disc, _, _) = run_instance_switch_scenario(&signal, &signal, SwitchType::Reused);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Switch boundary disc {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (reused engine, all-zero block, sine)"
    );
    assert!(
        resume_disc < MAX_BOUNDARY_DISC,
        "Resume boundary disc {resume_disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (reused engine, all-zero block, sine)"
    );
}

#[test]
fn pattern1_reused_allzero_music() {
    eprintln!("\n=== Pattern 1: Reused engine, all-zero block, music input ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = bandlimited_signal(total);
    let (disc, resume_disc, _, _) = run_instance_switch_scenario(&signal, &signal, SwitchType::Reused);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Switch boundary disc {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (reused engine, all-zero block, music)"
    );
    assert!(
        resume_disc < MAX_BOUNDARY_DISC,
        "Resume boundary disc {resume_disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (reused engine, all-zero block, music)"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Pattern 2: Fresh primed engine + all-zero first block
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pattern2_fresh_allzero_sine() {
    eprintln!("\n=== Pattern 2: Fresh primed engine, all-zero block, sine ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = sine_wave(440.0, total);
    let (disc, resume_disc, _, _) = run_instance_switch_scenario(&signal, &signal, SwitchType::FreshPrimed);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Switch boundary disc {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (fresh engine, all-zero block, sine)"
    );
    assert!(
        resume_disc < MAX_BOUNDARY_DISC,
        "Resume boundary disc {resume_disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (fresh engine, all-zero block, sine)"
    );
}

#[test]
fn pattern2_fresh_allzero_music() {
    eprintln!("\n=== Pattern 2: Fresh primed engine, all-zero block, music ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = bandlimited_signal(total);
    let (disc, resume_disc, _, _) = run_instance_switch_scenario(&signal, &signal, SwitchType::FreshPrimed);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Switch boundary disc {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (fresh engine, all-zero block, music)"
    );
    assert!(
        resume_disc < MAX_BOUNDARY_DISC,
        "Resume boundary disc {resume_disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (fresh engine, all-zero block, music)"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Pattern 3: Partial-zero first block (leading zeros + real audio)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn pattern3_reused_partial_zero_sine() {
    eprintln!("\n=== Pattern 3: Reused engine, partial-zero block, sine ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = sine_wave(440.0, total);
    let disc = run_partial_zero_scenario(&signal, &signal, SwitchType::Reused);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Boundary discontinuity {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (reused engine, partial-zero block, sine)"
    );
}

#[test]
fn pattern3_reused_partial_zero_music() {
    eprintln!("\n=== Pattern 3: Reused engine, partial-zero block, music ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = bandlimited_signal(total);
    let disc = run_partial_zero_scenario(&signal, &signal, SwitchType::Reused);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Boundary discontinuity {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (reused engine, partial-zero block, music)"
    );
}

#[test]
fn pattern3_fresh_partial_zero_music() {
    eprintln!("\n=== Pattern 3: Fresh primed engine, partial-zero block, music ===");
    let total = (WARMUP_BLOCKS + 60) * APO_BLOCK_SIZE;
    let signal = bandlimited_signal(total);
    let disc = run_partial_zero_scenario(&signal, &signal, SwitchType::FreshPrimed);
    assert!(
        disc < MAX_BOUNDARY_DISC,
        "Boundary discontinuity {disc:.6} exceeds threshold {MAX_BOUNDARY_DISC} \
         (fresh engine, partial-zero block, music)"
    );
}
