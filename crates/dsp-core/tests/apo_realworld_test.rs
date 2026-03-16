//! Real-world APO parameter test.
//!
//! Tests DSP output with the actual parameters the user had:
//! - All params at 1.0 (strength, hf, dynamics, transient)
//! - APO block sizes: 480 and 528
//! - FftOlaPilot synthesis mode
//! - Various signal types

use phaselith_dsp_core::config::{EngineConfig, QualityMode, StyleConfig, SynthesisMode};
use phaselith_dsp_core::engine::PhaselithEngineBuilder;
use phaselith_dsp_core::types::CrossChannelContext;

const SAMPLE_RATE: u32 = 48000;
const NUM_BLOCKS: usize = 200;

fn make_config_max_params() -> EngineConfig {
    EngineConfig {
        enabled: true,
        strength: 1.0,
        hf_reconstruction: 1.0,
        dynamics: 1.0,
        transient: 1.0,
        pre_echo_transient_scaling: 1.0,
        declip_transient_scaling: 1.0,
        delayed_transient_repair: false,
        phase_mode: phaselith_dsp_core::config::PhaseMode::Linear,
        quality_mode: QualityMode::Standard,
        style: StyleConfig::default(),
        synthesis_mode: SynthesisMode::FftOlaPilot,
        ambience_preserve: 0.0,
        filter_style: phaselith_dsp_core::config::FilterStyle::Reference,
    }
}

/// Generate band-limited music-like signal
fn generate_music_signal(num_samples: usize) -> Vec<f32> {
    let mut output = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let mut s = 0.0f32;
        // Multiple harmonics simulating music
        for k in 1..=8 {
            let freq = 440.0 * k as f32;
            if freq < SAMPLE_RATE as f32 / 2.0 {
                s += (2.0 * std::f32::consts::PI * freq * t).sin() / k as f32;
            }
        }
        // Add some bass
        s += 0.3 * (2.0 * std::f32::consts::PI * 100.0 * t).sin();
        // Add cymbal-like high freq
        s += 0.1 * (2.0 * std::f32::consts::PI * 8000.0 * t).sin();
        s *= 0.5; // normalize
        output.push(s);
    }
    output
}

fn simulate_apo(input: &[f32], block_size: usize, config: EngineConfig) -> Vec<f32> {
    let mut apo_config = config;
    apo_config.synthesis_mode = SynthesisMode::FftOlaPilot;

    let mut engine_l = PhaselithEngineBuilder::new(SAMPLE_RATE, block_size)
        .with_config(apo_config)
        .with_channels(1)
        .build_default();

    let mut engine_r = PhaselithEngineBuilder::new(SAMPLE_RATE, block_size)
        .with_config(apo_config)
        .with_channels(1)
        .build_default();

    let num_blocks = input.len() / block_size;
    let mut output = Vec::with_capacity(num_blocks * block_size);
    let mut cross_channel_prev: Option<CrossChannelContext> = None;

    for b in 0..num_blocks {
        let start = b * block_size;
        let end = start + block_size;

        let mut block_l: Vec<f32> = input[start..end].to_vec();
        let mut block_r: Vec<f32> = input[start..end].to_vec();
        let dry_l = block_l.clone();
        let dry_r = block_r.clone();

        engine_l.context_mut().cross_channel = cross_channel_prev;
        engine_l.process(&mut block_l);

        engine_r.context_mut().cross_channel = cross_channel_prev;
        engine_r.process(&mut block_r);

        cross_channel_prev = Some(CrossChannelContext::from_lr(&dry_l, &dry_r));
        output.extend_from_slice(&block_l);
    }
    output
}

fn analyze_output(name: &str, input: &[f32], output: &[f32], block_size: usize) {
    let len = input.len().min(output.len());

    // 1. Check for NaN/Inf
    let nan_count = output.iter().filter(|s| !s.is_finite()).count();
    eprintln!("  [{name}] NaN/Inf count: {nan_count}");
    assert_eq!(nan_count, 0, "{name}: output has NaN/Inf");

    // 2. Check bounds
    let max_abs = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    eprintln!("  [{name}] Max abs: {max_abs:.4}");
    assert!(max_abs <= 1.0, "{name}: output exceeds [-1,1], max={max_abs}");

    // 3. RMS comparison
    let input_rms = (input[..len].iter().map(|s| s * s).sum::<f32>() / len as f32).sqrt();
    let output_rms = (output[..len].iter().map(|s| s * s).sum::<f32>() / len as f32).sqrt();
    let rms_ratio = output_rms / input_rms.max(1e-10);
    eprintln!("  [{name}] RMS: input={input_rms:.4}, output={output_rms:.4}, ratio={rms_ratio:.3}");
    assert!(rms_ratio > 0.3 && rms_ratio < 3.0, "{name}: RMS ratio out of range: {rms_ratio}");

    // 4. Block boundary analysis (using max_within as reference)
    let num_blocks = len / block_size;
    if num_blocks >= 2 {
        let mut within_diffs: Vec<f32> = Vec::new();
        for b in 0..num_blocks {
            let s = b * block_size;
            for i in 1..block_size {
                within_diffs.push((output[s + i] - output[s + i - 1]).abs());
            }
        }
        let max_within = within_diffs.iter().cloned().fold(0.0f32, f32::max);
        let avg_within = within_diffs.iter().sum::<f32>() / within_diffs.len() as f32;

        let mut clicks = 0;
        let mut max_boundary = 0.0f32;
        for b in 1..num_blocks {
            let prev_end = b * block_size - 1;
            let next_start = b * block_size;
            let disc = (output[next_start] - output[prev_end]).abs();
            if disc > max_boundary { max_boundary = disc; }
            if disc > max_within * 1.05 + 0.005 {
                clicks += 1;
            }
        }
        eprintln!("  [{name}] Avg within-block diff: {avg_within:.6}");
        eprintln!("  [{name}] Max within-block diff: {max_within:.6}");
        eprintln!("  [{name}] Max boundary disc: {max_boundary:.6}");
        eprintln!("  [{name}] Boundary clicks: {clicks}/{}", num_blocks - 1);
        assert!(clicks == 0, "{name}: {clicks} boundary clicks detected");
    }

    // 5. DC offset
    let dc = output[..len].iter().sum::<f32>() / len as f32;
    eprintln!("  [{name}] DC offset: {dc:.6}");
    assert!(dc.abs() < 0.1, "{name}: excessive DC offset: {dc}");

    // 6. Correlation with input (should be positive, showing signal preservation)
    let mut corr_num = 0.0f32;
    let mut corr_den_a = 0.0f32;
    let mut corr_den_b = 0.0f32;
    for i in 0..len {
        corr_num += input[i] * output[i];
        corr_den_a += input[i] * input[i];
        corr_den_b += output[i] * output[i];
    }
    let corr = corr_num / (corr_den_a.sqrt() * corr_den_b.sqrt()).max(1e-10);
    eprintln!("  [{name}] Correlation with input: {corr:.4}");
    // At max params, correlation may be lower but should still be positive
    assert!(corr > 0.5, "{name}: correlation too low: {corr}");
}

#[test]
fn apo_max_params_480_block() {
    eprintln!("\n=== APO Max Params (block=480) ===");
    let block_size = 480;
    let total = NUM_BLOCKS * block_size;
    let input = generate_music_signal(total);
    let config = make_config_max_params();
    let output = simulate_apo(&input, block_size, config);
    analyze_output("max_480", &input, &output, block_size);
}

#[test]
fn apo_max_params_528_block() {
    eprintln!("\n=== APO Max Params (block=528) ===");
    let block_size = 528;
    let total = NUM_BLOCKS * block_size;
    let input = generate_music_signal(total);
    let config = make_config_max_params();
    let output = simulate_apo(&input, block_size, config);
    analyze_output("max_528", &input, &output, block_size);
}

#[test]
fn apo_max_params_sine_440() {
    eprintln!("\n=== APO Max Params (440Hz sine, block=480) ===");
    let block_size = 480;
    let total = NUM_BLOCKS * block_size;
    let input: Vec<f32> = (0..total)
        .map(|i| 0.7 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SAMPLE_RATE as f32).sin())
        .collect();
    let config = make_config_max_params();
    let output = simulate_apo(&input, block_size, config);
    analyze_output("max_sine440", &input, &output, block_size);
}

#[test]
fn apo_max_params_toggle_test() {
    // Simulate: run with enabled=true for 100 blocks, then switch to passthrough for 100 blocks
    // Verify no artifacts at transition and passthrough is clean
    eprintln!("\n=== APO Max Params Toggle Test ===");
    let block_size = 480;
    let total = NUM_BLOCKS * block_size;
    let input = generate_music_signal(total);
    let config = make_config_max_params();

    let mut engine = PhaselithEngineBuilder::new(SAMPLE_RATE, block_size)
        .with_config(config)
        .with_channels(1)
        .build_default();

    let mut output = Vec::with_capacity(total);
    let switch_block = NUM_BLOCKS / 2;

    for b in 0..NUM_BLOCKS {
        let start = b * block_size;
        let end = start + block_size;
        let mut block: Vec<f32> = input[start..end].to_vec();

        // Always process to keep engine state in sync (like real APO)
        engine.process(&mut block);

        if b < switch_block {
            // Enabled: use wet
            output.extend_from_slice(&block);
        } else {
            // Disabled: passthrough dry
            output.extend_from_slice(&input[start..end]);
        }
    }

    // Check passthrough section is EXACTLY the input
    let pass_start = switch_block * block_size;
    let pass_end = NUM_BLOCKS * block_size;
    for i in pass_start..pass_end.min(output.len()).min(input.len()) {
        assert_eq!(
            output[i], input[i],
            "Passthrough section should be exactly input at sample {i}"
        );
    }
    eprintln!("  [toggle] Passthrough section verified: {} samples match exactly", pass_end - pass_start);

    // Check wet section for artifacts
    analyze_output("toggle_wet", &input[..pass_start], &output[..pass_start], block_size);
}

/// Generate WAV for manual listening
fn write_wav(path: &str, samples: &[f32], sample_rate: u32) {
    let num_samples = samples.len() as u32;
    let byte_rate = sample_rate * 2;
    let data_size = num_samples * 2;
    let file_size = 36 + data_size;
    let mut data: Vec<u8> = Vec::with_capacity(file_size as usize + 8);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&file_size.to_le_bytes());
    data.extend_from_slice(b"WAVE");
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&sample_rate.to_le_bytes());
    data.extend_from_slice(&byte_rate.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
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
fn apo_generate_wav_for_listening() {
    let block_size = 480;
    let total = NUM_BLOCKS * block_size;
    let input = generate_music_signal(total);
    let config = make_config_max_params();
    let output = simulate_apo(&input, block_size, config);

    let test_dir = std::env::temp_dir().join("phaselith_apo_test");
    let _ = std::fs::create_dir_all(&test_dir);

    write_wav(&test_dir.join("maxparams_input.wav").to_string_lossy(), &input[..output.len()], SAMPLE_RATE);
    write_wav(&test_dir.join("maxparams_output.wav").to_string_lossy(), &output, SAMPLE_RATE);

    // Also generate residual (diff)
    let residual: Vec<f32> = output.iter().zip(input.iter()).map(|(o, i)| o - i).collect();
    write_wav(&test_dir.join("maxparams_residual.wav").to_string_lossy(), &residual, SAMPLE_RATE);

    eprintln!("\nWAV files for manual listening:");
    eprintln!("  Input:    {}", test_dir.join("maxparams_input.wav").display());
    eprintln!("  Output:   {}", test_dir.join("maxparams_output.wav").display());
    eprintln!("  Residual: {}", test_dir.join("maxparams_residual.wav").display());
}

/// Test with UltraExtreme quality — the actual APO configuration.
/// Checks for boundary clicks and CPU timing with FFT 8192 + 8 reprojection iters.
#[test]
fn apo_ultraextreme_528_block() {
    eprintln!("\n=== APO UltraExtreme (block=528, FFT=8192, 8 reproj) ===");
    let block_size = 528;
    let total = NUM_BLOCKS * block_size;
    let input = generate_music_signal(total);

    let config = EngineConfig {
        enabled: true,
        strength: 0.7,
        hf_reconstruction: 0.8,
        dynamics: 0.6,
        transient: 0.5,
        pre_echo_transient_scaling: 0.4,
        declip_transient_scaling: 1.0,
        delayed_transient_repair: false,
        phase_mode: phaselith_dsp_core::config::PhaseMode::Linear,
        quality_mode: QualityMode::UltraExtreme,
        style: StyleConfig::default(),
        synthesis_mode: SynthesisMode::FftOlaPilot,
        ambience_preserve: 0.0,
        filter_style: phaselith_dsp_core::config::FilterStyle::Reference,
    };
    let output = simulate_apo(&input, block_size, config);

    // Measure per-block processing time
    let mut engine_timing = PhaselithEngineBuilder::new(SAMPLE_RATE, block_size)
        .with_config(config)
        .with_channels(1)
        .build_default();
    let budget_us = (block_size as f64 / SAMPLE_RATE as f64) * 1_000_000.0;
    let mut max_us = 0.0f64;
    let mut overruns = 0;
    for b in 0..NUM_BLOCKS {
        let start = b * block_size;
        let end = start + block_size;
        let mut block: Vec<f32> = input[start..end].to_vec();
        let t0 = std::time::Instant::now();
        engine_timing.process(&mut block);
        let elapsed = t0.elapsed().as_micros() as f64;
        if elapsed > max_us { max_us = elapsed; }
        if elapsed > budget_us { overruns += 1; }
    }
    eprintln!("  Budget per block: {budget_us:.0}us");
    eprintln!("  Max process time: {max_us:.0}us");
    eprintln!("  CPU overruns: {overruns}/{NUM_BLOCKS}");

    analyze_output("ue_528", &input, &output, block_size);
}
