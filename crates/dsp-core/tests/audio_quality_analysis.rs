//! Comprehensive Audio Quality Analysis for APO
//!
//! Measures exactly what the engine does to audio signals:
//! - THD+N (Total Harmonic Distortion + Noise)
//! - Spectral difference before/after
//! - Block-by-block signal modification analysis
//! - Level stability over time
//!
//! Outputs detailed metrics and WAV files for manual inspection.

use phaselith_dsp_core::config::EngineConfig;
use phaselith_dsp_core::engine::PhaselithEngineBuilder;

const SR: u32 = 48000;
const BLOCK: usize = 480; // APO block size
const FFT: usize = 1024;  // Standard quality FFT size

/// Generate continuous sine wave.
fn sine(freq: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.7 * (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin()).collect()
}

/// Generate multi-tone signal (music-like harmonics).
fn multitone(n: usize) -> Vec<f32> {
    (0..n).map(|i| {
        let t = i as f32 / SR as f32;
        let mut s = 0.0f32;
        for &freq in &[440.0, 880.0, 1320.0, 1760.0, 3520.0, 7040.0] {
            s += 0.15 * (std::f32::consts::TAU * freq * t).sin();
        }
        s
    }).collect()
}

/// Compute RMS of a signal.
fn rms(signal: &[f32]) -> f32 {
    (signal.iter().map(|s| s * s).sum::<f32>() / signal.len().max(1) as f32).sqrt()
}

/// Compute signal difference metrics.
fn diff_metrics(input: &[f32], output: &[f32]) -> (f32, f32, f32) {
    let n = input.len().min(output.len());
    let diff: Vec<f32> = (0..n).map(|i| output[i] - input[i]).collect();
    let diff_rms = rms(&diff);
    let input_rms = rms(&input[..n]);
    let max_diff = diff.iter().map(|d| d.abs()).fold(0.0f32, f32::max);
    let snr_db = if diff_rms > 1e-10 {
        20.0 * (input_rms / diff_rms).log10()
    } else {
        f32::INFINITY
    };
    (diff_rms, max_diff, snr_db)
}

/// Simple DFT magnitude at specific bins (for THD measurement).
fn dft_magnitude_at(signal: &[f32], freq_hz: f32) -> f32 {
    let n = signal.len();
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for i in 0..n {
        let angle = std::f32::consts::TAU * freq_hz * i as f32 / SR as f32;
        re += signal[i] * angle.cos();
        im += signal[i] * (-angle).sin();
    }
    2.0 * (re * re + im * im).sqrt() / n as f32
}

/// Write WAV file (16-bit mono).
fn write_wav(path: &str, samples: &[f32]) {
    let n = samples.len() as u32;
    let data_size = n * 2;
    let file_size = 36 + data_size;
    let mut data = Vec::with_capacity(file_size as usize + 8);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&file_size.to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&SR.to_le_bytes());
    data.extend_from_slice(&(SR * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        data.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, data).unwrap();
}

/// Process signal through engine in APO-style 480-sample blocks.
fn process_apo_style(input: &[f32], config: EngineConfig) -> Vec<f32> {
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
    output
}

#[test]
fn quality_analysis_sine_440() {
    eprintln!("\n============================================================");
    eprintln!("=== Audio Quality: 440Hz Sine Wave ===");
    eprintln!("============================================================");

    let total = 200 * BLOCK; // ~2 sec
    let input = sine(440.0, total);
    let output = process_apo_style(&input, EngineConfig::default());

    let (diff_rms, max_diff, snr) = diff_metrics(&input, &output);
    eprintln!("  Input RMS:   {:.6}", rms(&input));
    eprintln!("  Output RMS:  {:.6}", rms(&output));
    eprintln!("  Diff RMS:    {:.6}", diff_rms);
    eprintln!("  Max diff:    {:.6}", max_diff);
    eprintln!("  SNR:         {:.1} dB", snr);

    // THD analysis: measure harmonics relative to fundamental
    // Use a segment from the middle (after engine warmup)
    let mid_start = 100 * BLOCK;
    let mid_end = mid_start + 10 * BLOCK;
    let segment = &output[mid_start..mid_end];
    let fundamental = dft_magnitude_at(segment, 440.0);
    let h2 = dft_magnitude_at(segment, 880.0);
    let h3 = dft_magnitude_at(segment, 1320.0);
    let h4 = dft_magnitude_at(segment, 1760.0);
    let h5 = dft_magnitude_at(segment, 2200.0);

    let thd = ((h2*h2 + h3*h3 + h4*h4 + h5*h5).sqrt() / fundamental.max(1e-10)) * 100.0;
    eprintln!("  Fundamental (440Hz): {:.6}", fundamental);
    eprintln!("  H2 (880Hz):          {:.6}  ({:.2}%)", h2, h2/fundamental.max(1e-10)*100.0);
    eprintln!("  H3 (1320Hz):         {:.6}  ({:.2}%)", h3, h3/fundamental.max(1e-10)*100.0);
    eprintln!("  H4 (1760Hz):         {:.6}  ({:.2}%)", h4, h4/fundamental.max(1e-10)*100.0);
    eprintln!("  H5 (2200Hz):         {:.6}  ({:.2}%)", h5, h5/fundamental.max(1e-10)*100.0);
    eprintln!("  THD:                 {:.2}%", thd);

    // Block-by-block level analysis
    let num_blocks = output.len() / BLOCK;
    let mut block_levels: Vec<f32> = Vec::new();
    for b in 0..num_blocks {
        let start = b * BLOCK;
        block_levels.push(rms(&output[start..start + BLOCK]));
    }
    let level_mean = block_levels.iter().sum::<f32>() / block_levels.len() as f32;
    let level_var = block_levels.iter().map(|l| (l - level_mean) * (l - level_mean)).sum::<f32>() / block_levels.len() as f32;
    let level_std = level_var.sqrt();
    eprintln!("  Block level mean:    {:.6}", level_mean);
    eprintln!("  Block level std:     {:.6}  ({:.2}% variation)", level_std, level_std/level_mean.max(1e-10)*100.0);

    // Write WAVs
    let dir = std::env::temp_dir().join("phaselith_quality");
    let _ = std::fs::create_dir_all(&dir);
    write_wav(&dir.join("sine_input.wav").to_string_lossy(), &input[..output.len()]);
    write_wav(&dir.join("sine_output.wav").to_string_lossy(), &output);
    let diff: Vec<f32> = output.iter().zip(input.iter()).map(|(o,i)| (o-i) * 10.0).collect(); // amplified 10x
    write_wav(&dir.join("sine_diff_10x.wav").to_string_lossy(), &diff);
    eprintln!("  WAVs: {}", dir.display());

    // Assertions
    assert!(snr > 20.0, "SNR too low: {snr:.1} dB");
    assert!(thd < 10.0, "THD too high: {thd:.2}%");
}

#[test]
fn quality_analysis_multitone() {
    eprintln!("\n============================================================");
    eprintln!("=== Audio Quality: Multi-tone (music-like) ===");
    eprintln!("============================================================");

    let total = 200 * BLOCK;
    let input = multitone(total);
    let output = process_apo_style(&input, EngineConfig::default());

    let (diff_rms, max_diff, snr) = diff_metrics(&input, &output);
    eprintln!("  Input RMS:   {:.6}", rms(&input));
    eprintln!("  Output RMS:  {:.6}", rms(&output));
    eprintln!("  Diff RMS:    {:.6}", diff_rms);
    eprintln!("  Max diff:    {:.6}", max_diff);
    eprintln!("  SNR:         {:.1} dB", snr);

    // Block-by-block difference analysis
    let num_blocks = output.len() / BLOCK;
    let mut per_block_snr: Vec<f32> = Vec::new();
    for b in 0..num_blocks {
        let s = b * BLOCK;
        let (_, _, bsnr) = diff_metrics(&input[s..s+BLOCK], &output[s..s+BLOCK]);
        per_block_snr.push(bsnr);
    }

    let worst_block = per_block_snr.iter().enumerate()
        .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(i, snr)| (i, *snr))
        .unwrap();
    let best_block = per_block_snr.iter().enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(i, snr)| (i, *snr))
        .unwrap();

    eprintln!("  Worst block SNR: block {} = {:.1} dB", worst_block.0, worst_block.1);
    eprintln!("  Best block SNR:  block {} = {:.1} dB", best_block.0, best_block.1);

    let dir = std::env::temp_dir().join("phaselith_quality");
    let _ = std::fs::create_dir_all(&dir);
    write_wav(&dir.join("multitone_input.wav").to_string_lossy(), &input[..output.len()]);
    write_wav(&dir.join("multitone_output.wav").to_string_lossy(), &output);
    let diff: Vec<f32> = output.iter().zip(input.iter()).map(|(o,i)| (o-i) * 10.0).collect();
    write_wav(&dir.join("multitone_diff_10x.wav").to_string_lossy(), &diff);
    eprintln!("  WAVs: {}", dir.display());

    assert!(snr > 15.0, "SNR too low: {snr:.1} dB");
}

#[test]
fn quality_analysis_zero_strength() {
    // With strength=0, the engine should be near-transparent
    eprintln!("\n============================================================");
    eprintln!("=== Audio Quality: Zero Strength (should be transparent) ===");
    eprintln!("============================================================");

    let total = 100 * BLOCK;
    let input = sine(1000.0, total);
    let mut config = EngineConfig::default();
    config.strength = 0.0;
    let output = process_apo_style(&input, config);

    let (diff_rms, max_diff, snr) = diff_metrics(&input, &output);
    eprintln!("  Diff RMS:  {:.6}", diff_rms);
    eprintln!("  Max diff:  {:.6}", max_diff);
    eprintln!("  SNR:       {:.1} dB", snr);

    // Zero strength should still apply warmth/smoothness from default style
    // Reference preset: warmth=0.15, smoothness=0.40
    // warmth: x - 0.15*0.15*x³/3 ≈ x (very subtle)
    // smoothness: (0.40-0.15)*0.12 = 0.03 → 3% smoothing
    eprintln!("  Note: warmth/smoothness still active from Reference style preset");

    assert!(snr > 30.0, "Zero-strength SNR too low: {snr:.1} dB — engine should be near-transparent");
}

#[test]
fn quality_analysis_no_character() {
    // With both strength=0 AND no character, should be truly transparent
    eprintln!("\n============================================================");
    eprintln!("=== Audio Quality: No Character (truly transparent) ===");
    eprintln!("============================================================");

    let total = 100 * BLOCK;
    let input = sine(1000.0, total);
    let mut config = EngineConfig::default();
    config.strength = 0.0;
    config.style = phaselith_dsp_core::config::StyleConfig::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let output = process_apo_style(&input, config);

    let (diff_rms, max_diff, snr) = diff_metrics(&input, &output);
    eprintln!("  Diff RMS:  {:.8}", diff_rms);
    eprintln!("  Max diff:  {:.8}", max_diff);
    eprintln!("  SNR:       {:.1} dB", snr);

    // Should be essentially bit-perfect (only level compensation might cause tiny changes)
    assert!(snr > 50.0, "Truly transparent mode SNR too low: {snr:.1} dB");
}

#[test]
fn quality_analysis_level_stability() {
    // Check that output level is stable over time (no pumping/modulation)
    eprintln!("\n============================================================");
    eprintln!("=== Audio Quality: Level Stability ===");
    eprintln!("============================================================");

    let total = 500 * BLOCK; // 5 seconds
    let input = sine(440.0, total);
    let output = process_apo_style(&input, EngineConfig::default());

    // Measure RMS per block
    let num_blocks = output.len() / BLOCK;
    let mut levels: Vec<f32> = Vec::new();
    for b in 0..num_blocks {
        levels.push(rms(&output[b*BLOCK..(b+1)*BLOCK]));
    }

    // Skip first 20 blocks (warmup)
    let stable = &levels[20..];
    let mean = stable.iter().sum::<f32>() / stable.len() as f32;
    let max_dev = stable.iter().map(|l| (l - mean).abs()).fold(0.0f32, f32::max);
    let max_dev_pct = max_dev / mean.max(1e-10) * 100.0;

    eprintln!("  Level mean (post-warmup): {:.6}", mean);
    eprintln!("  Max deviation:            {:.6} ({:.2}%)", max_dev, max_dev_pct);

    // Level should be very stable for a constant-amplitude sine
    assert!(max_dev_pct < 5.0, "Level instability: {max_dev_pct:.2}% — possible pumping");
}

#[test]
fn quality_analysis_effect_magnitude() {
    // Show exactly how much the engine changes the signal at default settings
    eprintln!("\n============================================================");
    eprintln!("=== Audio Quality: Effect Magnitude Summary ===");
    eprintln!("============================================================");

    let total = 200 * BLOCK;

    // Test different signal types
    let signals: Vec<(&str, Vec<f32>)> = vec![
        ("440Hz sine", sine(440.0, total)),
        ("1kHz sine", sine(1000.0, total)),
        ("Multi-tone", multitone(total)),
    ];

    for (name, input) in &signals {
        let output = process_apo_style(input, EngineConfig::default());
        let (diff_rms, max_diff, snr) = diff_metrics(input, &output);
        let level_change_db = 20.0 * (rms(&output) / rms(&input[..output.len()]).max(1e-10)).log10();

        eprintln!("  {name}:");
        eprintln!("    SNR: {snr:.1} dB | Max diff: {max_diff:.4} | Level change: {level_change_db:+.2} dB");
    }
}
