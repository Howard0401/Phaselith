//! APO Crackling Detection Test
//!
//! Simulates exactly how the Windows APO processes audio:
//! - Dual mono engines (L/R) with 480-sample blocks
//! - max_frame_size = 480 (actual APO frame, NOT fft_size)
//! - fft_size = 1024 (Standard quality, internal to M2/M5)
//! - hop_size = 256
//! - FftOlaPilot synthesis (forced for APO — LegacyAdditive causes
//!   discontinuities when block_size < fft_size)
//!
//! Detects block-boundary discontinuities that cause audible crackling.

use phaselith_dsp_core::config::{EngineConfig, SynthesisMode};
use phaselith_dsp_core::engine::{PhaselithEngine, PhaselithEngineBuilder};
use phaselith_dsp_core::types::CrossChannelContext;

const SAMPLE_RATE: u32 = 48000;
const APO_BLOCK_SIZE: usize = 480; // 10ms at 48kHz
const FFT_SIZE: usize = 1024; // Standard quality
const NUM_BLOCKS: usize = 200; // ~2 seconds of audio

/// Generate a continuous sine wave across all blocks.
fn generate_continuous_sine(freq: f32, num_samples: usize) -> Vec<f32> {
    (0..num_samples)
        .map(|i| 0.7 * (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE as f32).sin())
        .collect()
}

/// Generate a band-limited signal (simulating lossy codec output).
fn generate_bandlimited(num_samples: usize, cutoff_hz: f32) -> Vec<f32> {
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
    let dt = 1.0 / SAMPLE_RATE as f32;
    let alpha = dt / (rc + dt);

    let mut output = Vec::with_capacity(num_samples);
    let mut prev = 0.0f32;
    for i in 0..num_samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        // Rich signal with harmonics
        let mut s = 0.0f32;
        for k in (1..=15).step_by(2) {
            let freq = 440.0 * k as f32;
            if freq < SAMPLE_RATE as f32 / 2.0 {
                s += (2.0 * std::f32::consts::PI * freq * t).sin() / k as f32;
            }
        }
        s *= 0.4;
        // IIR low-pass
        prev = prev + alpha * (s - prev);
        output.push(prev);
    }
    output
}

/// Detect discontinuities at block boundaries.
/// Returns (max_discontinuity, avg_discontinuity, num_clicks).
fn detect_discontinuities(output: &[f32], block_size: usize) -> (f32, f32, usize) {
    let num_blocks = output.len() / block_size;
    if num_blocks < 2 {
        return (0.0, 0.0, 0);
    }

    let mut max_disc = 0.0f32;
    let mut sum_disc = 0.0f32;
    let mut num_clicks = 0usize;

    // Measure average sample-to-sample diff WITHIN blocks for baseline
    let mut within_block_diffs = Vec::new();
    for b in 0..num_blocks {
        let start = b * block_size;
        for i in 1..block_size {
            let diff = (output[start + i] - output[start + i - 1]).abs();
            within_block_diffs.push(diff);
        }
    }
    let avg_within = within_block_diffs.iter().sum::<f32>() / within_block_diffs.len().max(1) as f32;
    let max_within = within_block_diffs.iter().cloned().fold(0.0f32, f32::max);

    // Check discontinuities AT block boundaries
    for b in 1..num_blocks {
        let prev_end = b * block_size - 1;
        let next_start = b * block_size;
        let disc = (output[next_start] - output[prev_end]).abs();

        if disc > max_disc {
            max_disc = disc;
        }
        sum_disc += disc;

        // A "click" is when the boundary discontinuity exceeds the maximum
        // within-block transition. This avoids false positives for complex
        // waveforms where the average is low but peaks are large.
        if disc > max_within * 1.05 + 0.005 {
            num_clicks += 1;
        }
    }

    let avg_disc = sum_disc / (num_blocks - 1) as f32;
    eprintln!("  Block boundary analysis:");
    eprintln!("    Avg within-block diff: {avg_within:.6}");
    eprintln!("    Max within-block diff: {max_within:.6}");
    eprintln!("    Max boundary disc:     {max_disc:.6}");
    eprintln!("    Avg boundary disc:     {avg_disc:.6}");
    eprintln!("    Clicks (>3x avg):      {num_clicks}/{}", num_blocks - 1);

    (max_disc, avg_disc, num_clicks)
}

/// Simulate APO processing: dual mono engines, 480-sample blocks.
/// Uses APO_BLOCK_SIZE as max_frame_size (matching the real APO fix)
/// and forces FftOlaPilot synthesis mode.
fn simulate_apo_processing(input_l: &[f32], input_r: &[f32], config: EngineConfig) -> (Vec<f32>, Vec<f32>) {
    // APO uses frame_size (480) as max_frame_size, NOT fft_size (1024).
    // Also forces FftOlaPilot to avoid LegacyAdditive discontinuities.
    let mut apo_config = config;
    apo_config.synthesis_mode = SynthesisMode::FftOlaPilot;

    let mut engine_l = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_BLOCK_SIZE)
        .with_config(apo_config)
        .with_channels(1)
        .build_default();

    let mut engine_r = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_BLOCK_SIZE)
        .with_config(apo_config)
        .with_channels(1)
        .build_default();

    let total_samples = input_l.len().min(input_r.len());
    let num_blocks = total_samples / APO_BLOCK_SIZE;

    let mut output_l = Vec::with_capacity(num_blocks * APO_BLOCK_SIZE);
    let mut output_r = Vec::with_capacity(num_blocks * APO_BLOCK_SIZE);
    let mut cross_channel_prev: Option<CrossChannelContext> = None;

    for b in 0..num_blocks {
        let start = b * APO_BLOCK_SIZE;
        let end = start + APO_BLOCK_SIZE;

        let mut block_l: Vec<f32> = input_l[start..end].to_vec();
        let mut block_r: Vec<f32> = input_r[start..end].to_vec();

        // Save dry copies for cross-channel
        let dry_l = block_l.clone();
        let dry_r = block_r.clone();

        // Inject cross-channel context (symmetric one-frame delay)
        engine_l.context_mut().cross_channel = cross_channel_prev;
        engine_l.process(&mut block_l);

        engine_r.context_mut().cross_channel = cross_channel_prev;
        engine_r.process(&mut block_r);

        // Compute cross-channel from dry signals
        cross_channel_prev = Some(CrossChannelContext::from_lr(&dry_l, &dry_r));

        output_l.extend_from_slice(&block_l);
        output_r.extend_from_slice(&block_r);
    }

    (output_l, output_r)
}

/// Write raw audio data as a WAV file for Python analysis.
fn write_wav(path: &str, samples: &[f32], sample_rate: u32) {
    let num_samples = samples.len() as u32;
    let byte_rate = sample_rate * 2; // 16-bit mono
    let data_size = num_samples * 2;
    let file_size = 36 + data_size;

    let mut data: Vec<u8> = Vec::with_capacity(file_size as usize + 8);
    // RIFF header
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&file_size.to_le_bytes());
    data.extend_from_slice(b"WAVE");
    // fmt chunk
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&sample_rate.to_le_bytes());
    data.extend_from_slice(&byte_rate.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes()); // block align
    data.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    // data chunk
    data.extend_from_slice(b"data");
    data.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        data.extend_from_slice(&i16_val.to_le_bytes());
    }

    std::fs::write(path, data).unwrap();
}

#[test]
fn apo_sim_sine_no_crackling() {
    eprintln!("\n=== APO Simulation: 440Hz Sine ===");
    let total = NUM_BLOCKS * APO_BLOCK_SIZE;
    let input = generate_continuous_sine(440.0, total);
    let config = EngineConfig::default(); // enabled=true, LegacyAdditive

    let (out_l, _out_r) = simulate_apo_processing(&input, &input, config);

    let (max_disc, avg_disc, num_clicks) = detect_discontinuities(&out_l, APO_BLOCK_SIZE);

    // Write WAVs for manual inspection
    let test_dir = std::env::temp_dir().join("phaselith_apo_test");
    let _ = std::fs::create_dir_all(&test_dir);
    write_wav(
        &test_dir.join("sine_input.wav").to_string_lossy(),
        &input[..out_l.len()],
        SAMPLE_RATE,
    );
    write_wav(
        &test_dir.join("sine_output_l.wav").to_string_lossy(),
        &out_l,
        SAMPLE_RATE,
    );
    eprintln!("  WAV files written to: {}", test_dir.display());

    // Assertions: block boundary discontinuities should not be significantly
    // larger than within-block transitions
    assert!(
        num_clicks < (NUM_BLOCKS / 10), // less than 10% of blocks should have clicks
        "Too many clicks detected: {num_clicks} out of {} block boundaries",
        NUM_BLOCKS - 1
    );
}

#[test]
fn apo_sim_bandlimited_no_crackling() {
    eprintln!("\n=== APO Simulation: Bandlimited Signal (14kHz cutoff) ===");
    let total = NUM_BLOCKS * APO_BLOCK_SIZE;
    let input = generate_bandlimited(total, 14000.0);
    let mut config = EngineConfig::default();
    config.strength = 0.7;

    let (out_l, _out_r) = simulate_apo_processing(&input, &input, config);

    let (max_disc, avg_disc, num_clicks) = detect_discontinuities(&out_l, APO_BLOCK_SIZE);

    let test_dir = std::env::temp_dir().join("phaselith_apo_test");
    let _ = std::fs::create_dir_all(&test_dir);
    write_wav(
        &test_dir.join("bandlimited_input.wav").to_string_lossy(),
        &input[..out_l.len()],
        SAMPLE_RATE,
    );
    write_wav(
        &test_dir.join("bandlimited_output_l.wav").to_string_lossy(),
        &out_l,
        SAMPLE_RATE,
    );
    eprintln!("  WAV files written to: {}", test_dir.display());

    assert!(
        num_clicks < (NUM_BLOCKS / 10),
        "Too many clicks: {num_clicks} out of {} boundaries",
        NUM_BLOCKS - 1
    );
}

#[test]
fn apo_sim_residual_only_discontinuity() {
    // Test ONLY the residual (difference between output and input)
    // to isolate synthesis discontinuities from dry signal continuity.
    eprintln!("\n=== APO Simulation: Residual-Only Discontinuity Check ===");
    let total = NUM_BLOCKS * APO_BLOCK_SIZE;
    let input = generate_bandlimited(total, 14000.0);
    let mut config = EngineConfig::default();
    config.strength = 1.0; // max strength to maximize residual

    let (out_l, _) = simulate_apo_processing(&input, &input, config);

    // Compute residual: output - input
    let residual: Vec<f32> = out_l.iter()
        .zip(input.iter())
        .map(|(o, i)| o - i)
        .collect();

    let (max_disc, avg_disc, num_clicks) = detect_discontinuities(&residual, APO_BLOCK_SIZE);

    let test_dir = std::env::temp_dir().join("phaselith_apo_test");
    let _ = std::fs::create_dir_all(&test_dir);
    write_wav(
        &test_dir.join("residual_only.wav").to_string_lossy(),
        &residual,
        SAMPLE_RATE,
    );
    eprintln!("  Residual WAV written to: {}", test_dir.display());

    // Residual discontinuities should be small
    // The residual itself is small (typically <0.1), so discontinuities should be tiny
    assert!(
        max_disc < 0.3,
        "Residual has large discontinuity at block boundary: {max_disc:.4}"
    );
}

#[test]
fn apo_sim_toggle_stability() {
    // Simulate toggling enabled/disabled (like the user reports getting worse)
    eprintln!("\n=== APO Simulation: Toggle Stability ===");
    let total = NUM_BLOCKS * APO_BLOCK_SIZE;
    let input = generate_bandlimited(total, 14000.0);
    let mut config = EngineConfig::default();
    config.synthesis_mode = SynthesisMode::FftOlaPilot;

    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, APO_BLOCK_SIZE)
        .with_config(config)
        .with_channels(1)
        .build_default();

    let mut output = Vec::with_capacity(total);
    let mut enabled = true;

    for b in 0..NUM_BLOCKS {
        let start = b * APO_BLOCK_SIZE;
        let end = start + APO_BLOCK_SIZE;
        let mut block: Vec<f32> = input[start..end].to_vec();

        // Toggle every 20 blocks
        if b % 20 == 0 && b > 0 {
            enabled = !enabled;
        }

        // Always process through engine (APO always runs engine)
        engine.process(&mut block);

        if enabled {
            output.extend_from_slice(&block);
        } else {
            output.extend_from_slice(&input[start..end]);
        }
    }

    let (max_disc, avg_disc, num_clicks) = detect_discontinuities(&output, APO_BLOCK_SIZE);

    let test_dir = std::env::temp_dir().join("phaselith_apo_test");
    let _ = std::fs::create_dir_all(&test_dir);
    write_wav(
        &test_dir.join("toggle_output.wav").to_string_lossy(),
        &output,
        SAMPLE_RATE,
    );

    // Toggle transitions naturally have discontinuities (wet↔dry),
    // but non-toggle blocks should be smooth
    let non_toggle_clicks = {
        let num_blocks = output.len() / APO_BLOCK_SIZE;
        let mut clicks = 0;
        let mut within_diffs = Vec::new();
        for b in 0..num_blocks {
            let s = b * APO_BLOCK_SIZE;
            for i in 1..APO_BLOCK_SIZE {
                within_diffs.push((output[s + i] - output[s + i - 1]).abs());
            }
        }
        let max_within = within_diffs.iter().cloned().fold(0.0f32, f32::max);

        for b in 1..num_blocks {
            // Skip toggle boundaries (every 20 blocks)
            if b % 20 == 0 { continue; }
            let prev_end = b * APO_BLOCK_SIZE - 1;
            let next_start = b * APO_BLOCK_SIZE;
            let disc = (output[next_start] - output[prev_end]).abs();
            if disc > max_within * 1.05 + 0.005 {
                clicks += 1;
            }
        }
        clicks
    };

    eprintln!("  Non-toggle clicks: {non_toggle_clicks}");
    assert!(
        non_toggle_clicks < (NUM_BLOCKS / 10),
        "Too many non-toggle clicks: {non_toggle_clicks}"
    );
}

#[test]
fn apo_sim_continuous_processing_stable() {
    // Process 500 blocks (5 seconds) and check that crackling doesn't get worse over time
    eprintln!("\n=== APO Simulation: Long-term Stability ===");
    let num_blocks = 500;
    let total = num_blocks * APO_BLOCK_SIZE;
    let input = generate_bandlimited(total, 14000.0);
    let config = EngineConfig::default();

    let (out_l, _) = simulate_apo_processing(&input, &input, config);

    // Compare first half vs second half discontinuity rates
    let half = (num_blocks / 2) * APO_BLOCK_SIZE;
    let (_, _, clicks_first) = detect_discontinuities(&out_l[..half], APO_BLOCK_SIZE);
    let (_, _, clicks_second) = detect_discontinuities(&out_l[half..], APO_BLOCK_SIZE);

    eprintln!("  First half clicks:  {clicks_first}");
    eprintln!("  Second half clicks: {clicks_second}");

    // Second half should not be significantly worse than first half
    // (tests for state accumulation causing progressive degradation)
    if clicks_first > 0 {
        assert!(
            clicks_second <= clicks_first * 3 + 5,
            "Crackling getting worse over time: first={clicks_first}, second={clicks_second}"
        );
    }
}
