//! Integration test simulating the APO audio processing flow.
//!
//! Verifies that the DSP engine produces valid output when processing
//! blocks of 528 samples (Windows APO frame size) with FftOlaPilot
//! synthesis mode, matching the exact configuration used by the APO DLL.
//!
//! Run with: cargo test --package phaselith-dsp-core --test apo_audio_safety_test

use phaselith_dsp_core::config::{EngineConfig, QualityMode, StyleConfig, SynthesisMode};
use phaselith_dsp_core::engine::PhaselithEngineBuilder;
use std::f32::consts::PI;

/// APO-typical block size (from Windows audio engine descriptor).
const APO_FRAME_SIZE: usize = 528;
const SAMPLE_RATE: u32 = 48000;

/// Number of blocks to process for warmup + steady-state analysis.
const NUM_BLOCKS: usize = 200;

/// Create an EngineConfig matching what the APO DLL uses.
fn apo_config() -> EngineConfig {
    EngineConfig {
        enabled: true,
        strength: 0.7,
        hf_reconstruction: 0.8,
        dynamics: 0.6,
        transient: 0.5,
        pre_echo_transient_scaling: 1.0,
        declip_transient_scaling: 1.0,
        delayed_transient_repair: false,
        phase_mode: phaselith_dsp_core::config::PhaseMode::Linear,
        quality_mode: QualityMode::Standard,
        style: StyleConfig::default(),
        synthesis_mode: SynthesisMode::FftOlaPilot,
        ambience_preserve: 0.0,
    }
}

/// Generate a 440Hz sine wave block.
fn sine_block(offset: usize) -> Vec<f32> {
    (0..APO_FRAME_SIZE)
        .map(|i| {
            let n = (offset + i) as f32;
            0.5 * (2.0 * PI * 440.0 * n / SAMPLE_RATE as f32).sin()
        })
        .collect()
}

/// Generate a band-limited test signal (harmonics up to ~8kHz).
fn bandlimited_block(offset: usize) -> Vec<f32> {
    (0..APO_FRAME_SIZE)
        .map(|i| {
            let t = (offset + i) as f32 / SAMPLE_RATE as f32;
            let mut s = 0.0f32;
            for k in (1..=15).step_by(2) {
                let freq = 440.0 * k as f32;
                if freq < 8000.0 {
                    s += (2.0 * PI * freq * t).sin() / k as f32;
                }
            }
            s * 0.3
        })
        .collect()
}

#[test]
fn apo_engine_output_is_finite() {
    // Verify every sample is finite (no NaN/Inf) across many blocks.
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    for block_idx in 0..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);

        for (i, &s) in samples.iter().enumerate() {
            assert!(
                s.is_finite(),
                "NaN/Inf at block {} sample {}: value={}",
                block_idx, i, s
            );
        }
    }
}

#[test]
fn apo_engine_output_within_bounds() {
    // Verify output doesn't exceed [-1.0, 1.0] (true peak guard).
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    let mut max_abs = 0.0f32;
    for block_idx in 0..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);

        for &s in &samples {
            max_abs = max_abs.max(s.abs());
        }
    }

    assert!(
        max_abs <= 1.0,
        "Output exceeded [-1.0, 1.0]: max_abs={}",
        max_abs
    );
}

#[test]
fn apo_engine_preserves_dry_signal_character() {
    // Verify the output resembles the input (high correlation).
    // The engine should ADD a small residual, not replace the signal.
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    // Skip warmup blocks
    for block_idx in 0..100 {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);
    }

    // Measure correlation on steady-state blocks
    let mut correlation_sum = 0.0f64;
    let mut dry_energy = 0.0f64;
    let mut wet_energy = 0.0f64;

    for block_idx in 100..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let dry = sine_block(offset);
        let mut wet = dry.clone();
        engine.process(&mut wet);

        for i in 0..APO_FRAME_SIZE {
            correlation_sum += (dry[i] as f64) * (wet[i] as f64);
            dry_energy += (dry[i] as f64) * (dry[i] as f64);
            wet_energy += (wet[i] as f64) * (wet[i] as f64);
        }
    }

    let correlation = correlation_sum / (dry_energy.sqrt() * wet_energy.sqrt());
    assert!(
        correlation > 0.9,
        "Output should correlate strongly with input (enhancement, not replacement). \
         Correlation = {:.4}",
        correlation
    );
}

#[test]
fn apo_engine_no_dc_offset() {
    // Verify the output doesn't develop a DC offset (common OLA bug).
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    // Process warmup
    for block_idx in 0..100 {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);
    }

    // Measure DC on steady-state blocks
    let mut dc_sum = 0.0f64;
    let mut sample_count = 0u64;

    for block_idx in 100..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);

        for &s in &samples {
            dc_sum += s as f64;
            sample_count += 1;
        }
    }

    let dc_mean = dc_sum / sample_count as f64;
    assert!(
        dc_mean.abs() < 0.05,
        "Output has DC offset: mean = {:.6}",
        dc_mean
    );
}

#[test]
fn apo_engine_rms_within_reasonable_range() {
    // Verify the output RMS is within 2x of the input RMS.
    // This catches cases where the engine amplifies or attenuates excessively.
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    // Skip warmup
    for block_idx in 0..100 {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);
    }

    let mut dry_rms_sum = 0.0f64;
    let mut wet_rms_sum = 0.0f64;
    let blocks = NUM_BLOCKS - 100;

    for block_idx in 100..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let dry = sine_block(offset);
        let mut wet = dry.clone();
        engine.process(&mut wet);

        let dry_rms: f64 = dry.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / APO_FRAME_SIZE as f64;
        let wet_rms: f64 = wet.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / APO_FRAME_SIZE as f64;

        dry_rms_sum += dry_rms;
        wet_rms_sum += wet_rms;
    }

    let avg_dry_rms = (dry_rms_sum / blocks as f64).sqrt();
    let avg_wet_rms = (wet_rms_sum / blocks as f64).sqrt();
    let ratio = avg_wet_rms / avg_dry_rms;

    assert!(
        ratio > 0.5 && ratio < 2.0,
        "Output RMS deviates too much from input: dry_rms={:.4}, wet_rms={:.4}, ratio={:.4}",
        avg_dry_rms, avg_wet_rms, ratio
    );
}

#[test]
fn apo_engine_dual_mono_symmetric() {
    // Verify L and R engines produce consistent results (no divergence).
    let config = apo_config();
    let mut engine_l = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();
    let mut engine_r = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    // Feed identical signal to both engines
    for block_idx in 0..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let dry = sine_block(offset);
        let mut wet_l = dry.clone();
        let mut wet_r = dry.clone();

        engine_l.process(&mut wet_l);
        engine_r.process(&mut wet_r);

        // Both engines should produce identical output for identical input
        for i in 0..APO_FRAME_SIZE {
            let diff = (wet_l[i] - wet_r[i]).abs();
            assert!(
                diff < 1e-5,
                "L/R divergence at block {} sample {}: L={} R={} diff={}",
                block_idx, i, wet_l[i], wet_r[i], diff
            );
        }
    }
}

#[test]
fn apo_engine_block_boundary_continuity() {
    // Verify there's no discontinuity at block boundaries.
    // Measures the jump between the last sample of one block and
    // the first sample of the next. Large jumps = clicks/pops.
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    let mut prev_last_sample = 0.0f32;
    let mut max_jump = 0.0f32;

    for block_idx in 0..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = sine_block(offset);
        engine.process(&mut samples);

        if block_idx > 10 {
            // Skip warmup blocks
            let jump = (samples[0] - prev_last_sample).abs();
            max_jump = max_jump.max(jump);
        }
        prev_last_sample = samples[APO_FRAME_SIZE - 1];
    }

    // For a 440Hz sine at 48kHz, the max natural inter-sample difference is:
    // 2*π*440/48000 * 0.5 ≈ 0.029. Allow up to 0.2 for enhancement effects.
    assert!(
        max_jump < 0.2,
        "Block boundary discontinuity too large: max_jump={:.4} \
         (indicates clicking/popping artifacts)",
        max_jump
    );
}

#[test]
fn apo_engine_bandlimited_signal_quality() {
    // Process a band-limited signal (simulating lossy codec output).
    // Verify the engine doesn't destroy it.
    let config = apo_config();
    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_FRAME_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    for block_idx in 0..NUM_BLOCKS {
        let offset = block_idx * APO_FRAME_SIZE;
        let mut samples = bandlimited_block(offset);
        engine.process(&mut samples);

        // Every sample must be finite
        for (i, &s) in samples.iter().enumerate() {
            assert!(
                s.is_finite(),
                "NaN/Inf in bandlimited test at block {} sample {}",
                block_idx, i
            );
        }

        // Peak should stay within bounds
        let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak <= 1.0,
            "Peak exceeded 1.0 in bandlimited test at block {}: peak={}",
            block_idx, peak
        );
    }
}
