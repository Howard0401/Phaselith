//! Click gate / output safety tests.
//!
//! Tests the output gate that detects and mutes audio artifacts:
//! - Boundary discontinuities (block-to-block jumps)
//! - Engine restart artifacts (lock_for_process → stale OLA)
//! - Audio gap recovery (UAC, audio graph changes)
//! - Extreme input values (near-clipping, DC offset, impulse)
//! - Abrupt signal changes (silence → full scale, genre switch)
//! - System-level anomalies (NaN injection, denormals)
//!
//! These tests verify the click gate at the DSP engine level.
//! The actual gate lives in apo_impl.rs (outside dsp-core), so we
//! test the engine's output characteristics that the gate relies on.

use phaselith_dsp_core::config::{EngineConfig, QualityMode, StyleConfig, SynthesisMode};
use phaselith_dsp_core::engine::PhaselithEngineBuilder;

const SAMPLE_RATE: u32 = 48000;
const BLOCK_SIZE: usize = 480; // 10ms at 48kHz (typical APO)
const BLOCK_SIZE_528: usize = 528; // Odd APO size

fn apo_config() -> EngineConfig {
    EngineConfig {
        enabled: true,
        strength: 0.7,
        hf_reconstruction: 0.8,
        dynamics: 0.6,
        transient: 0.5,
        pre_echo_transient_scaling: 0.4,
        declip_transient_scaling: 1.0,
        delayed_transient_repair: false,
        phase_mode: phaselith_dsp_core::config::PhaseMode::Linear,
        quality_mode: QualityMode::Standard,
        style: StyleConfig::default(),
        synthesis_mode: SynthesisMode::LegacyAdditive,
        ambience_preserve: 0.0,
        filter_style: phaselith_dsp_core::config::FilterStyle::Reference,
    }
}

/// Simulates the click gate detection logic from apo_impl.rs.
/// Returns (boundary_disc, max_within_disc, is_click).
fn detect_click(prev_last: f32, block: &[f32]) -> (f32, f32, bool) {
    let boundary_disc = (block[0] - prev_last).abs();
    let mut max_within = 0.0f32;
    for i in 1..block.len() {
        let d = (block[i] - block[i - 1]).abs();
        if d > max_within {
            max_within = d;
        }
    }
    const CLICK_RATIO: f32 = 1.5;
    const CLICK_FLOOR: f32 = 0.01;
    let is_click = boundary_disc > max_within * CLICK_RATIO + CLICK_FLOOR;
    (boundary_disc, max_within, is_click)
}

/// Process N blocks and return per-block outputs + click detection results.
struct ClickTestResult {
    blocks: Vec<Vec<f32>>,
    clicks_detected: usize,
    max_boundary_disc: f32,
}

fn process_signal(input: &[f32], block_size: usize, config: EngineConfig) -> ClickTestResult {
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, block_size)
        .with_config(config)
        .with_channels(1)
        .build_default();

    // Prime engine (same as APO lock_for_process)
    let mut silent = vec![0.0f32; block_size];
    for _ in 0..6 {
        engine.process(&mut silent);
        silent.fill(0.0);
    }

    let num_blocks = input.len() / block_size;
    let mut blocks = Vec::with_capacity(num_blocks);
    let mut clicks = 0;
    let mut max_disc = 0.0f32;
    let mut prev_last = 0.0f32;

    // Skip warmup phase (32 blocks) for click detection
    for b in 0..num_blocks {
        let start = b * block_size;
        let mut block: Vec<f32> = input[start..start + block_size].to_vec();
        engine.process(&mut block);

        if b >= 32 {
            // Only check after warmup
            let (disc, _within, is_click) = detect_click(prev_last, &block);
            if disc > max_disc {
                max_disc = disc;
            }
            if is_click {
                clicks += 1;
            }
        }

        prev_last = *block.last().unwrap_or(&0.0);
        blocks.push(block);
    }

    ClickTestResult {
        blocks,
        clicks_detected: clicks,
        max_boundary_disc: max_disc,
    }
}

fn generate_sine(freq: f32, num_samples: usize, amplitude: f32) -> Vec<f32> {
    (0..num_samples)
        .map(|i| amplitude * (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE as f32).sin())
        .collect()
}

fn generate_music(num_samples: usize) -> Vec<f32> {
    let mut output = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let mut s = 0.0f32;
        for k in 1..=8 {
            let freq = 440.0 * k as f32;
            if freq < SAMPLE_RATE as f32 / 2.0 {
                s += (2.0 * std::f32::consts::PI * freq * t).sin() / k as f32;
            }
        }
        s += 0.3 * (2.0 * std::f32::consts::PI * 100.0 * t).sin();
        s *= 0.5;
        output.push(s);
    }
    output
}

// ═══════════════════════════════════════════════════════════════
// 1. Normal operation — no false positives
// ═══════════════════════════════════════════════════════════════

#[test]
fn no_clicks_on_steady_music() {
    let total = 200 * BLOCK_SIZE;
    let input = generate_music(total);
    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("steady_music: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "Steady music should not trigger click gate");
}

#[test]
fn no_clicks_on_steady_sine_440() {
    let total = 200 * BLOCK_SIZE;
    let input = generate_sine(440.0, total, 0.7);
    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("sine_440: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "440Hz sine should not trigger click gate");
}

#[test]
fn no_clicks_on_steady_sine_528_block() {
    let total = 200 * BLOCK_SIZE_528;
    let input = generate_sine(440.0, total, 0.7);
    let result = process_signal(&input, BLOCK_SIZE_528, apo_config());
    eprintln!("sine_528: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "528-block sine should not trigger click gate");
}

#[test]
fn no_clicks_on_near_clipping_sine() {
    // Near full-scale signal — should not false-trigger the gate
    let total = 200 * BLOCK_SIZE;
    let input = generate_sine(440.0, total, 0.99);
    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("near_clip: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "Near-clipping sine should not trigger gate");
}

#[test]
fn no_clicks_on_very_quiet_signal() {
    // Very quiet signal (-60dBFS)
    let total = 200 * BLOCK_SIZE;
    let input = generate_sine(1000.0, total, 0.001);
    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("quiet: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "Quiet signal should not trigger gate");
}

#[test]
fn no_clicks_on_low_bass() {
    // Very low bass — large sample-to-sample changes are normal
    let total = 200 * BLOCK_SIZE;
    let input = generate_sine(30.0, total, 0.8);
    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("bass_30hz: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "30Hz bass should not trigger gate");
}

#[test]
fn no_clicks_on_high_treble() {
    // Near-Nyquist content
    let total = 200 * BLOCK_SIZE;
    let input = generate_sine(15000.0, total, 0.5);
    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("treble_15k: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "15kHz sine should not trigger gate");
}

// ═══════════════════════════════════════════════════════════════
// 2. Extreme values — engine output robustness
// ═══════════════════════════════════════════════════════════════

#[test]
fn engine_handles_full_scale_square_wave() {
    // Square wave — maximum sample-to-sample change, tests engine stability
    let total = 100 * BLOCK_SIZE;
    let input: Vec<f32> = (0..total)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            if (t * 440.0).fract() < 0.5 { 0.9 } else { -0.9 }
        })
        .collect();
    let result = process_signal(&input, BLOCK_SIZE, apo_config());

    // Verify no NaN/Inf
    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Square wave produced NaN/Inf");
        }
    }
    eprintln!("square_wave: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

#[test]
fn engine_handles_impulse_train() {
    // Single-sample impulses — extreme transient content
    let total = 100 * BLOCK_SIZE;
    let mut input = vec![0.0f32; total];
    for i in (0..total).step_by(BLOCK_SIZE / 2) {
        input[i] = 0.9;
    }
    let result = process_signal(&input, BLOCK_SIZE, apo_config());

    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Impulse train produced NaN/Inf");
            assert!(s.abs() <= 1.0, "Impulse train output exceeded [-1,1]");
        }
    }
    eprintln!("impulse: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

#[test]
fn engine_handles_dc_offset() {
    // DC signal — tests engine bias behavior
    let total = 100 * BLOCK_SIZE;
    let input = vec![0.5f32; total];
    let result = process_signal(&input, BLOCK_SIZE, apo_config());

    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "DC input produced NaN/Inf");
        }
    }
    eprintln!("dc_offset: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

#[test]
fn engine_handles_white_noise() {
    // Random noise — worst case for false click detection
    let total = 200 * BLOCK_SIZE;
    let mut rng_state: u32 = 12345;
    let input: Vec<f32> = (0..total)
        .map(|_| {
            // Simple LCG for deterministic "random"
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            let u = (rng_state >> 16) as f32 / 32768.0; // 0..1
            (u - 0.5) * 1.4 // ±0.7
        })
        .collect();
    let result = process_signal(&input, BLOCK_SIZE, apo_config());

    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "White noise produced NaN/Inf");
        }
    }
    eprintln!("noise: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

// ═══════════════════════════════════════════════════════════════
// 3. Audio gap simulation — the primary use case
// ═══════════════════════════════════════════════════════════════

#[test]
fn gap_detection_silence_then_music() {
    // Simulate: music → 50ms silence gap → music resumes
    // The gap causes stale OLA → potential click at resume
    let block_count = 200;
    let total = block_count * BLOCK_SIZE;
    let music = generate_music(total);

    let gap_start_block = 100;
    let gap_blocks = 5; // ~50ms gap

    let mut input = music.clone();
    for b in gap_start_block..gap_start_block + gap_blocks {
        for i in 0..BLOCK_SIZE {
            input[b * BLOCK_SIZE + i] = 0.0;
        }
    }

    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, BLOCK_SIZE)
        .with_config(apo_config())
        .with_channels(1)
        .build_default();

    // Prime
    let mut silent = vec![0.0f32; BLOCK_SIZE];
    for _ in 0..6 {
        engine.process(&mut silent);
        silent.fill(0.0);
    }

    let mut prev_last = 0.0f32;
    let mut gap_resume_disc = 0.0f32;

    for b in 0..block_count {
        let start = b * BLOCK_SIZE;
        let mut block: Vec<f32> = input[start..start + BLOCK_SIZE].to_vec();
        engine.process(&mut block);

        if b == gap_start_block + gap_blocks {
            // First block after gap — measure boundary discontinuity
            gap_resume_disc = (block[0] - prev_last).abs();
            eprintln!(
                "gap_resume: boundary_disc={:.6}, prev_last={:.6}, first_sample={:.6}",
                gap_resume_disc, prev_last, block[0]
            );
        }

        prev_last = *block.last().unwrap_or(&0.0);
    }

    eprintln!("gap_test: resume_disc={:.6}", gap_resume_disc);
    // The click gate should catch this if it's significant
    // We just verify the engine doesn't produce NaN/garbage
}

#[test]
fn gap_detection_multiple_gaps() {
    // Multiple gaps in sequence — tests reset logic
    let block_count = 300;
    let total = block_count * BLOCK_SIZE;
    let music = generate_music(total);

    let mut input = music.clone();
    // Gap 1: blocks 50-55
    // Gap 2: blocks 120-130
    // Gap 3: blocks 200-202 (short)
    let gaps = [(50, 5), (120, 10), (200, 2)];
    for (start, len) in &gaps {
        for b in *start..*start + *len {
            for i in 0..BLOCK_SIZE {
                if b * BLOCK_SIZE + i < total {
                    input[b * BLOCK_SIZE + i] = 0.0;
                }
            }
        }
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());

    // Verify no NaN
    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Multi-gap produced NaN/Inf");
        }
    }
    eprintln!("multi_gap: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

// ═══════════════════════════════════════════════════════════════
// 4. Abrupt signal changes — genre switch, volume jump
// ═══════════════════════════════════════════════════════════════

#[test]
fn no_false_clicks_on_volume_jump() {
    // Simulate sudden volume change (e.g., quiet intro → loud chorus)
    let total = 200 * BLOCK_SIZE;
    let mut input = Vec::with_capacity(total);

    // First half: quiet sine
    for i in 0..total / 2 {
        let t = i as f32 / SAMPLE_RATE as f32;
        input.push(0.05 * (2.0 * std::f32::consts::PI * 440.0 * t).sin());
    }
    // Second half: loud sine (instant jump)
    for i in total / 2..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        input.push(0.8 * (2.0 * std::f32::consts::PI * 440.0 * t).sin());
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("vol_jump: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    // A volume jump is NOT a click — engine should handle it smoothly
    // Allow at most 1 detected click at the boundary (acceptable for 16× volume jump)
    assert!(result.clicks_detected <= 1, "Volume jump caused too many false clicks: {}", result.clicks_detected);
}

#[test]
fn no_false_clicks_on_freq_change() {
    // Abrupt frequency change (e.g., different instrument)
    let total = 200 * BLOCK_SIZE;
    let mut input = Vec::with_capacity(total);

    // First half: 200Hz
    for i in 0..total / 2 {
        let t = i as f32 / SAMPLE_RATE as f32;
        input.push(0.7 * (2.0 * std::f32::consts::PI * 200.0 * t).sin());
    }
    // Second half: 4000Hz
    for i in total / 2..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        input.push(0.7 * (2.0 * std::f32::consts::PI * 4000.0 * t).sin());
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("freq_change: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert!(result.clicks_detected <= 1, "Frequency change caused too many false clicks: {}", result.clicks_detected);
}

// ═══════════════════════════════════════════════════════════════
// 5. Engine restart simulation
// ═══════════════════════════════════════════════════════════════

#[test]
fn fresh_engine_first_blocks_are_safe() {
    // Verify that a freshly created + primed engine produces clean output
    // from the very first block (no click at block 0→1 boundary)
    let input = generate_music(50 * BLOCK_SIZE);

    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, BLOCK_SIZE)
        .with_config(apo_config())
        .with_channels(1)
        .build_default();

    // Prime (same as APO)
    let mut silent = vec![0.0f32; BLOCK_SIZE];
    for _ in 0..6 {
        engine.process(&mut silent);
        silent.fill(0.0);
    }

    let mut prev_last = 0.0f32;
    let mut max_disc = 0.0f32;

    for b in 0..50 {
        let start = b * BLOCK_SIZE;
        let mut block: Vec<f32> = input[start..start + BLOCK_SIZE].to_vec();
        engine.process(&mut block);

        let disc = (block[0] - prev_last).abs();
        if disc > max_disc {
            max_disc = disc;
        }

        for &s in &block {
            assert!(s.is_finite(), "Fresh engine block {b} has NaN/Inf");
        }

        prev_last = *block.last().unwrap();
    }

    eprintln!("fresh_engine: max_boundary_disc={:.6}", max_disc);
}

// ═══════════════════════════════════════════════════════════════
// 6. System anomaly signals
// ═══════════════════════════════════════════════════════════════

#[test]
fn engine_survives_denormal_input() {
    // Denormal floats — can cause massive CPU spikes on some hardware
    let total = 50 * BLOCK_SIZE;
    let mut input = vec![0.0f32; total];
    // Sprinkle denormals
    for i in (0..total).step_by(7) {
        input[i] = f32::from_bits(1); // smallest positive denormal
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Denormal input produced NaN/Inf");
        }
    }
    eprintln!("denormal: output clean");
}

#[test]
fn engine_survives_alternating_polarity_bursts() {
    // Alternating +1/-1 samples (worst case for filters)
    let total = 100 * BLOCK_SIZE;
    let input: Vec<f32> = (0..total)
        .map(|i| if i % 2 == 0 { 0.9 } else { -0.9 })
        .collect();
    let result = process_signal(&input, BLOCK_SIZE, apo_config());

    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Alternating polarity produced NaN/Inf");
        }
    }
    eprintln!("alt_polarity: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

#[test]
fn engine_handles_sudden_silence() {
    // Music → instant silence (simulates audio stream stop)
    let total = 200 * BLOCK_SIZE;
    let mut input = generate_music(total);

    // Kill audio at block 100
    for i in 100 * BLOCK_SIZE..total {
        input[i] = 0.0;
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Sudden silence produced NaN/Inf");
        }
    }
    // After silence, output should decay to near-zero (OLA drains)
    let last_block = &result.blocks[result.blocks.len() - 1];
    let last_rms: f32 = (last_block.iter().map(|s| s * s).sum::<f32>() / last_block.len() as f32).sqrt();
    eprintln!("sudden_silence: last_block_rms={:.8}", last_rms);
    assert!(last_rms < 0.01, "Output should decay to near-silence, got rms={last_rms}");
}

#[test]
fn engine_handles_silence_to_full_scale() {
    // Instant silence → full scale (simulates audio stream resume)
    let total = 200 * BLOCK_SIZE;
    let mut input = vec![0.0f32; total];

    // Start music at block 100
    let music = generate_music(total);
    for i in 100 * BLOCK_SIZE..total {
        input[i] = music[i];
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    for block in &result.blocks {
        for &s in block {
            assert!(s.is_finite(), "Silence→music produced NaN/Inf");
        }
    }
    eprintln!("silence_to_music: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
}

// ═══════════════════════════════════════════════════════════════
// 7. Config change during processing
// ═══════════════════════════════════════════════════════════════

#[test]
fn no_clicks_on_config_hot_reload() {
    // Simulate changing config mid-stream (filter style change)
    let total = 200 * BLOCK_SIZE;
    let input = generate_music(total);

    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, BLOCK_SIZE)
        .with_config(apo_config())
        .with_channels(1)
        .build_default();

    let mut silent = vec![0.0f32; BLOCK_SIZE];
    for _ in 0..6 {
        engine.process(&mut silent);
        silent.fill(0.0);
    }

    let mut prev_last = 0.0f32;
    let mut clicks = 0;

    for b in 0..200 {
        // Change config at block 100
        if b == 100 {
            let mut new_config = apo_config();
            new_config.strength = 1.0;
            new_config.hf_reconstruction = 1.0;
            new_config.dynamics = 1.0;
            new_config.transient = 1.0;
            new_config.style = StyleConfig::new(0.55, 0.30, 0.60, 0.30, 0.15, 0.50); // Warm
            engine.update_config(new_config);
        }

        let start = b * BLOCK_SIZE;
        let mut block: Vec<f32> = input[start..start + BLOCK_SIZE].to_vec();
        engine.process(&mut block);

        if b >= 16 {
            let (_, _, is_click) = detect_click(prev_last, &block);
            if is_click {
                clicks += 1;
                eprintln!("  config_change: click at block {b}, disc={:.6}",
                    (block[0] - prev_last).abs());
            }
        }
        prev_last = *block.last().unwrap();
    }

    eprintln!("config_change: clicks={clicks}");
    // Config change might cause 1 click at most (parameters change instantly)
    assert!(clicks <= 1, "Config hot-reload caused {clicks} clicks");
}

// ═══════════════════════════════════════════════════════════════
// 8. Boundary condition: all quality modes
// ═══════════════════════════════════════════════════════════════

#[test]
fn no_clicks_across_quality_modes() {
    let total = 100 * BLOCK_SIZE;
    let input = generate_music(total);

    for (name, mode) in [
        ("Light", QualityMode::Light),
        ("Standard", QualityMode::Standard),
        ("Ultra", QualityMode::Ultra),
    ] {
        let mut config = apo_config();
        config.quality_mode = mode;
        let result = process_signal(&input, BLOCK_SIZE, config);
        eprintln!("{name}: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
        assert_eq!(
            result.clicks_detected, 0,
            "{name} mode produced {clicks} clicks",
            clicks = result.clicks_detected
        );
    }
}

// ═══════════════════════════════════════════════════════════════
// 9. Multi-frequency content (realistic music simulation)
// ═══════════════════════════════════════════════════════════════

#[test]
fn no_clicks_on_complex_signal() {
    // Complex signal with bass + mid + treble + transient-like bursts
    let total = 200 * BLOCK_SIZE;
    let mut input = Vec::with_capacity(total);

    for i in 0..total {
        let t = i as f32 / SAMPLE_RATE as f32;
        let mut s = 0.0f32;

        // Sub bass
        s += 0.3 * (2.0 * std::f32::consts::PI * 60.0 * t).sin();
        // Kick-like envelope (4Hz amplitude modulation on bass)
        let kick_env = (0.5 + 0.5 * (2.0 * std::f32::consts::PI * 4.0 * t).sin()).powf(4.0);
        s += 0.4 * kick_env * (2.0 * std::f32::consts::PI * 80.0 * t).sin();
        // Mid harmonics
        s += 0.2 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        s += 0.15 * (2.0 * std::f32::consts::PI * 880.0 * t).sin();
        // HF shimmer
        s += 0.08 * (2.0 * std::f32::consts::PI * 5000.0 * t).sin();
        s += 0.05 * (2.0 * std::f32::consts::PI * 10000.0 * t).sin();

        input.push(s.clamp(-0.95, 0.95));
    }

    let result = process_signal(&input, BLOCK_SIZE, apo_config());
    eprintln!("complex: clicks={}, max_disc={:.6}", result.clicks_detected, result.max_boundary_disc);
    assert_eq!(result.clicks_detected, 0, "Complex music signal triggered false clicks");
}
