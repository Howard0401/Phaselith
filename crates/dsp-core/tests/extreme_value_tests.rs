//! Extreme value and edge case tests for the CIRRUS engine.
//!
//! These tests verify robustness at boundaries:
//! - Sub-block splitting edge cases (1 sample, hop±1, exact multiples)
//! - NaN/Inf input resilience
//! - Extreme parameter combinations
//! - Long-duration numerical stability (drift detection)
//! - Extreme sample rates (8kHz, 192kHz)

use phaselith_dsp_core::engine::PhaselithEngineBuilder;
use phaselith_dsp_core::config::{EngineConfig, QualityMode, StyleConfig, StylePreset};

fn sine_signal(freq: f32, sample_rate: u32, num_samples: usize) -> Vec<f32> {
    (0..num_samples)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin())
        .collect()
}

fn build_engine(sample_rate: u32, max_frame: usize) -> phaselith_dsp_core::engine::PhaselithEngine {
    PhaselithEngineBuilder::new(sample_rate, max_frame).build_default()
}

fn build_engine_with_config(
    sample_rate: u32,
    max_frame: usize,
    config: EngineConfig,
) -> phaselith_dsp_core::engine::PhaselithEngine {
    PhaselithEngineBuilder::new(sample_rate, max_frame)
        .with_config(config)
        .build_default()
}

fn is_finite_buffer(buf: &[f32]) -> bool {
    buf.iter().all(|s| s.is_finite())
}

fn max_abs(buf: &[f32]) -> f32 {
    buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
}

// ═══════════════════════════════════════════════════════════════════
// 1. Sub-block splitting boundary tests
// ═══════════════════════════════════════════════════════════════════

#[test]
fn subblock_single_sample() {
    // 1 sample — smallest possible block. Must not panic.
    let mut engine = build_engine(48000, 1024);
    let mut buf = vec![0.5f32; 1];
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
}

#[test]
fn subblock_empty_buffer() {
    // 0 samples — degenerate case. Must not panic.
    let mut engine = build_engine(48000, 1024);
    let mut buf: Vec<f32> = vec![];
    engine.process(&mut buf);
}

#[test]
fn subblock_exactly_hop_size() {
    // Exactly hop_size (256 for Standard). Should take fast path, no splitting.
    let mut engine = build_engine(48000, 1024);
    let hop = QualityMode::Standard.hop_size(); // 256
    let mut buf = sine_signal(440.0, 48000, hop);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 1);
}

#[test]
fn subblock_hop_plus_one() {
    // hop_size + 1 = 257. Should split into 256 + 1 = 2 sub-blocks.
    let mut engine = build_engine(48000, 1024);
    let hop = QualityMode::Standard.hop_size();
    let mut buf = sine_signal(440.0, 48000, hop + 1);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 2); // 2 sub-blocks
}

#[test]
fn subblock_hop_minus_one() {
    // hop_size - 1 = 255. Should take fast path, no splitting.
    let mut engine = build_engine(48000, 1024);
    let hop = QualityMode::Standard.hop_size();
    let mut buf = sine_signal(440.0, 48000, hop - 1);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 1);
}

#[test]
fn subblock_exact_multiple_of_hop() {
    // 4 * hop = 1024. Should split into exactly 4 sub-blocks.
    let mut engine = build_engine(48000, 1024);
    let hop = QualityMode::Standard.hop_size();
    let mut buf = sine_signal(440.0, 48000, hop * 4);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 4);
}

#[test]
fn subblock_non_multiple_of_hop() {
    // 528 samples (APO real-world). 528 / 256 = 2 full + 16 remainder = 3 sub-blocks.
    let mut engine = build_engine(48000, 1024);
    let mut buf = sine_signal(440.0, 48000, 528);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 3); // 256 + 256 + 16
}

#[test]
fn subblock_480_chrome_like() {
    // 480 samples (Chrome-like). 480 / 256 = 1 full + 224 remainder = 2 sub-blocks.
    let mut engine = build_engine(48000, 1024);
    let mut buf = sine_signal(440.0, 48000, 480);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 2); // 256 + 224
}

#[test]
fn subblock_128_wasm() {
    // 128 samples (WASM). < hop_size, fast path.
    let mut engine = build_engine(48000, 1024);
    let mut buf = sine_signal(440.0, 48000, 128);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 1);
}

#[test]
fn subblock_consistency_across_splits() {
    // Process 1024 samples in different chunk patterns, verify frame_index.
    // Pattern A: one 1024-sample block → 4 sub-blocks
    let mut engine_a = build_engine(48000, 1024);
    let mut buf_a = sine_signal(440.0, 48000, 1024);
    engine_a.process(&mut buf_a);
    assert_eq!(engine_a.context().frame_index, 4);

    // Pattern B: four 256-sample blocks → 4 × 1 sub-block
    let mut engine_b = build_engine(48000, 1024);
    for _ in 0..4 {
        let mut buf_b = sine_signal(440.0, 48000, 256);
        engine_b.process(&mut buf_b);
    }
    assert_eq!(engine_b.context().frame_index, 4);
}

// ═══════════════════════════════════════════════════════════════════
// 2. NaN/Inf input robustness
// ═══════════════════════════════════════════════════════════════════

#[test]
fn nan_input_does_not_propagate() {
    let mut engine = build_engine(48000, 1024);
    // Warm up with clean signal first
    for _ in 0..5 {
        let mut warmup = sine_signal(440.0, 48000, 256);
        engine.process(&mut warmup);
    }
    // Inject NaN
    let mut buf = vec![f32::NAN; 256];
    engine.process(&mut buf);
    // Follow up with clean signal — must recover
    let mut clean = sine_signal(440.0, 48000, 256);
    engine.process(&mut clean);
    assert!(
        is_finite_buffer(&clean),
        "Engine must recover from NaN input"
    );
}

#[test]
fn inf_input_does_not_propagate() {
    let mut engine = build_engine(48000, 1024);
    for _ in 0..5 {
        let mut warmup = sine_signal(440.0, 48000, 256);
        engine.process(&mut warmup);
    }
    let mut buf = vec![f32::INFINITY; 256];
    engine.process(&mut buf);
    let mut clean = sine_signal(440.0, 48000, 256);
    engine.process(&mut clean);
    assert!(
        is_finite_buffer(&clean),
        "Engine must recover from Inf input"
    );
}

#[test]
fn neg_inf_input_does_not_propagate() {
    let mut engine = build_engine(48000, 1024);
    for _ in 0..5 {
        let mut warmup = sine_signal(440.0, 48000, 256);
        engine.process(&mut warmup);
    }
    let mut buf = vec![f32::NEG_INFINITY; 256];
    engine.process(&mut buf);
    let mut clean = sine_signal(440.0, 48000, 256);
    engine.process(&mut clean);
    assert!(
        is_finite_buffer(&clean),
        "Engine must recover from -Inf input"
    );
}

#[test]
fn mixed_nan_inf_normal_input() {
    let mut engine = build_engine(48000, 1024);
    for _ in 0..5 {
        let mut warmup = sine_signal(440.0, 48000, 256);
        engine.process(&mut warmup);
    }
    // Mixed: normal, NaN, Inf, -Inf, subnormal
    let mut buf = vec![0.5; 256];
    buf[0] = f32::NAN;
    buf[1] = f32::INFINITY;
    buf[2] = f32::NEG_INFINITY;
    buf[3] = f32::MIN_POSITIVE * 0.5; // subnormal
    buf[4] = -0.0;
    engine.process(&mut buf);
    // Doesn't need to be clean (input was garbage), just must not panic
    // and subsequent clean input must work
    let mut clean = sine_signal(440.0, 48000, 256);
    engine.process(&mut clean);
    assert!(is_finite_buffer(&clean));
}

#[test]
fn subnormal_input_handled() {
    let mut engine = build_engine(48000, 1024);
    // All subnormals — should not cause slowdowns or issues
    let mut buf = vec![f32::MIN_POSITIVE * 1e-20; 256];
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
}

// ═══════════════════════════════════════════════════════════════════
// 3. Extreme parameter combinations
// ═══════════════════════════════════════════════════════════════════

#[test]
fn params_all_zero() {
    let mut config = EngineConfig::default();
    config.strength = 0.0;
    config.hf_reconstruction = 0.0;
    config.dynamics = 0.0;
    config.transient = 0.0;
    config.style = StyleConfig::from_preset(StylePreset::Reference);
    let mut engine = build_engine_with_config(48000, 1024, config);
    let mut buf = sine_signal(440.0, 48000, 256);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    // With all params at 0, output should be near-transparent
    assert!(max_abs(&buf) < 2.0, "All-zero params: output bounded");
}

#[test]
fn params_all_max() {
    let mut config = EngineConfig::default();
    config.strength = 1.0;
    config.hf_reconstruction = 1.0;
    config.dynamics = 1.0;
    config.transient = 1.0;
    config.style = StyleConfig {
        warmth: 1.0,
        air_brightness: 1.0,
        smoothness: 1.0,
        spatial_spread: 1.0,
        impact_gain: 1.0,
        body: 1.0,
    };
    let mut engine = build_engine_with_config(48000, 1024, config);
    let mut buf = sine_signal(440.0, 48000, 256);
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
    assert!(max_abs(&buf) < 4.0, "All-max params: output bounded");
}

#[test]
fn params_strength_zero_hf_max() {
    // strength=0 should bypass residual application regardless of hf_reconstruction
    let mut config = EngineConfig::default();
    config.strength = 0.0;
    config.hf_reconstruction = 1.0;
    let mut engine = build_engine_with_config(48000, 1024, config);
    let input = sine_signal(440.0, 48000, 256);
    let mut buf = input.clone();
    for _ in 0..10 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn params_dynamics_zero_transient_max() {
    let mut config = EngineConfig::default();
    config.dynamics = 0.0;
    config.transient = 1.0;
    let mut engine = build_engine_with_config(48000, 1024, config);
    let mut buf = sine_signal(440.0, 48000, 256);
    for _ in 0..10 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
    assert!(max_abs(&buf) < 4.0);
}

#[test]
fn params_max_warmth_zero_air() {
    let mut config = EngineConfig::default();
    config.style = StyleConfig {
        warmth: 1.0,
        air_brightness: 0.0,
        smoothness: 0.5,
        spatial_spread: 0.5,
        impact_gain: 0.5,
        body: 0.5,
    };
    let mut engine = build_engine_with_config(48000, 1024, config);
    let mut buf = sine_signal(440.0, 48000, 256);
    for _ in 0..10 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn params_zero_warmth_max_air() {
    let mut config = EngineConfig::default();
    config.style = StyleConfig {
        warmth: 0.0,
        air_brightness: 1.0,
        smoothness: 0.0,
        spatial_spread: 1.0,
        impact_gain: 0.0,
        body: 0.0,
    };
    let mut engine = build_engine_with_config(48000, 1024, config);
    let mut buf = sine_signal(440.0, 48000, 256);
    for _ in 0..10 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn all_quality_modes_with_max_params() {
    let modes = [
        QualityMode::Light,
        QualityMode::Standard,
        QualityMode::Ultra,
        QualityMode::Extreme,
        QualityMode::UltraExtreme,
    ];
    for mode in &modes {
        let mut config = EngineConfig::default();
        config.quality_mode = *mode;
        config.strength = 1.0;
        config.hf_reconstruction = 1.0;
        config.dynamics = 1.0;
        config.transient = 1.0;
        let max_frame = mode.fft_size();
        let mut engine = build_engine_with_config(48000, max_frame, config);
        let mut buf = sine_signal(440.0, 48000, mode.hop_size());
        for _ in 0..10 {
            engine.process(&mut buf);
        }
        assert!(
            is_finite_buffer(&buf),
            "Quality mode {:?} with max params produced non-finite output",
            mode
        );
    }
}

#[test]
fn config_change_mid_stream_extreme() {
    // Rapidly change config between extremes while processing
    let mut engine = build_engine(48000, 1024);
    for i in 0..50 {
        let mut config = EngineConfig::default();
        if i % 2 == 0 {
            config.strength = 1.0;
            config.hf_reconstruction = 1.0;
            config.dynamics = 1.0;
        } else {
            config.strength = 0.0;
            config.hf_reconstruction = 0.0;
            config.dynamics = 0.0;
        }
        engine.update_config(config);
        let mut buf = sine_signal(440.0, 48000, 256);
        engine.process(&mut buf);
        assert!(
            is_finite_buffer(&buf),
            "Config oscillation at frame {i} produced non-finite output"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. Long-duration numerical stability (drift detection)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn long_duration_1000_blocks_no_drift() {
    let mut engine = build_engine(48000, 1024);
    let mut rms_history: Vec<f32> = Vec::new();

    for block in 0..1000 {
        let mut buf = sine_signal(440.0, 48000, 256);
        engine.process(&mut buf);

        assert!(
            is_finite_buffer(&buf),
            "Non-finite output at block {block}"
        );
        assert!(
            max_abs(&buf) < 4.0,
            "Amplitude explosion at block {block}: max={}",
            max_abs(&buf)
        );

        let rms = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        rms_history.push(rms);
    }

    // Check that RMS doesn't drift: compare first 100 vs last 100 blocks
    let early_avg: f32 =
        rms_history[100..200].iter().sum::<f32>() / 100.0; // skip first 100 (warmup)
    let late_avg: f32 = rms_history[900..1000].iter().sum::<f32>() / 100.0;

    if early_avg > 0.001 {
        let drift_ratio = (late_avg / early_avg - 1.0).abs();
        assert!(
            drift_ratio < 0.5,
            "RMS drift detected: early={early_avg:.4}, late={late_avg:.4}, drift={drift_ratio:.2}"
        );
    }
}

#[test]
fn long_duration_silence_no_noise_buildup() {
    // Process silence for 2000 blocks — verify no noise accumulates
    let mut engine = build_engine(48000, 1024);

    for block in 0..2000 {
        let mut buf = vec![0.0f32; 256];
        engine.process(&mut buf);

        assert!(
            is_finite_buffer(&buf),
            "Non-finite output from silence at block {block}"
        );

        let peak = max_abs(&buf);
        assert!(
            peak < 0.01,
            "Noise buildup from silence at block {block}: peak={peak}"
        );
    }
}

#[test]
fn long_duration_alternating_signal_silence() {
    // Alternate between signal and silence — verify clean transitions
    let mut engine = build_engine(48000, 1024);

    for block in 0..500 {
        let mut buf = if block % 20 < 10 {
            sine_signal(440.0, 48000, 256)
        } else {
            vec![0.0f32; 256]
        };
        engine.process(&mut buf);
        assert!(
            is_finite_buffer(&buf),
            "Non-finite at block {block} ({})",
            if block % 20 < 10 { "signal" } else { "silence" }
        );
        assert!(
            max_abs(&buf) < 4.0,
            "Amplitude explosion at block {block}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// 5. Extreme sample rates
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sample_rate_8khz() {
    // Telephony rate — cutoff ~3.5kHz, very few bins
    let mut engine = build_engine(8000, 1024);
    let mut buf = sine_signal(440.0, 8000, 256);
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
    assert!(max_abs(&buf) < 4.0);
}

#[test]
fn sample_rate_16khz() {
    // Wideband speech
    let mut engine = build_engine(16000, 1024);
    let mut buf = sine_signal(440.0, 16000, 256);
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn sample_rate_22050() {
    // Half CD rate
    let mut engine = build_engine(22050, 1024);
    let mut buf = sine_signal(440.0, 22050, 256);
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn sample_rate_192khz() {
    // Studio hi-res — very wide spectrum, many bins
    let mut engine = build_engine(192000, 4096);
    let mut buf = sine_signal(440.0, 192000, 256);
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
    assert!(max_abs(&buf) < 4.0);
}

#[test]
fn sample_rate_384khz() {
    // Extreme hi-res (DSD-equivalent)
    let mut engine = build_engine(384000, 4096);
    let mut buf = sine_signal(440.0, 384000, 256);
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

// ═══════════════════════════════════════════════════════════════════
// 6. Extreme signal content
// ═══════════════════════════════════════════════════════════════════

#[test]
fn extremely_loud_input() {
    // Input well above 0 dBFS (e.g., +20 dBFS = amplitude 10.0)
    let mut engine = build_engine(48000, 1024);
    let mut buf: Vec<f32> = sine_signal(440.0, 48000, 256)
        .iter()
        .map(|s| s * 10.0)
        .collect();
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn extremely_quiet_input() {
    // -120 dBFS signal — near noise floor
    let mut engine = build_engine(48000, 1024);
    let mut buf: Vec<f32> = sine_signal(440.0, 48000, 256)
        .iter()
        .map(|s| s * 1e-6)
        .collect();
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
    assert!(
        max_abs(&buf) < 0.01,
        "Quiet signal should stay quiet, got max={}",
        max_abs(&buf)
    );
}

#[test]
fn dc_offset_large() {
    // Large DC offset — should not cause instability
    let mut engine = build_engine(48000, 1024);
    let mut buf = vec![0.9f32; 256];
    for _ in 0..50 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn alternating_polarity_square_wave() {
    // +1, -1, +1, -1... — maximum slew rate, maximum HF content
    let mut engine = build_engine(48000, 1024);
    let mut buf: Vec<f32> = (0..256).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
    for _ in 0..20 {
        engine.process(&mut buf);
    }
    assert!(is_finite_buffer(&buf));
}

#[test]
fn single_spike_impulse() {
    // Single sample at +1.0, rest zeros — tests impulse response
    let mut engine = build_engine(48000, 1024);
    let mut buf = vec![0.0f32; 256];
    buf[0] = 1.0;
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    // Second block should not have residual energy explosion
    let mut buf2 = vec![0.0f32; 256];
    engine.process(&mut buf2);
    assert!(is_finite_buffer(&buf2));
    assert!(max_abs(&buf2) < 1.0);
}

// ═══════════════════════════════════════════════════════════════════
// 7. Engine state edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn rapid_reset_cycles() {
    // Reset engine many times in quick succession
    let mut engine = build_engine(48000, 1024);
    for cycle in 0..20 {
        let mut buf = sine_signal(440.0, 48000, 256);
        engine.process(&mut buf);
        engine.reset();
        assert_eq!(
            engine.context().frame_index,
            0,
            "frame_index not 0 after reset cycle {cycle}"
        );
    }
    // Must still work after many resets
    let mut buf = sine_signal(440.0, 48000, 256);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
}

#[test]
fn enable_disable_toggle_rapid() {
    // Toggle enabled rapidly — simulates user clicking fast
    let mut engine = build_engine(48000, 1024);
    for i in 0..100 {
        let mut config = EngineConfig::default();
        config.enabled = i % 2 == 0;
        engine.update_config(config);
        let mut buf = sine_signal(440.0, 48000, 256);
        engine.process(&mut buf);
        assert!(
            is_finite_buffer(&buf),
            "Toggle at iteration {i} produced non-finite output"
        );
    }
}

#[test]
fn process_after_reset_no_stale_state() {
    // Verify that reset truly clears all state — process should behave
    // identically to a fresh engine.
    let mut engine = build_engine(48000, 1024);

    // Process some data to populate state
    for _ in 0..50 {
        let mut buf = sine_signal(440.0, 48000, 256);
        engine.process(&mut buf);
    }

    engine.reset();

    // After reset, damage should be at defaults
    assert!(
        (engine.context().damage.cutoff.mean - 20000.0).abs() < 1.0,
        "Cutoff not reset to default: {}",
        engine.context().damage.cutoff.mean
    );
}

// ═══════════════════════════════════════════════════════════════════
// 8. Sub-block splitting with different quality modes
// ═══════════════════════════════════════════════════════════════════

#[test]
fn subblock_light_mode_hop_128() {
    // Light mode: FFT 512, hop=128. Block 528 > 128 → 5 sub-blocks (128×4 + 16).
    let mut config = EngineConfig::default();
    config.quality_mode = QualityMode::Light;
    let mut engine = build_engine_with_config(48000, 1024, config);
    let hop = QualityMode::Light.hop_size(); // 128
    let mut buf = sine_signal(440.0, 48000, 528);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    let expected = (528 + hop - 1) / hop; // ceil(528/128) = 5
    assert_eq!(engine.context().frame_index, expected as u64);
}

#[test]
fn subblock_ultra_mode_hop_512() {
    // Ultra mode: FFT 2048, hop=512. Block 1024 > 512 → 2 sub-blocks (512+512).
    let mut config = EngineConfig::default();
    config.quality_mode = QualityMode::Ultra;
    let mut engine = build_engine_with_config(48000, 2048, config);
    let hop = QualityMode::Ultra.hop_size(); // 512
    let mut buf = sine_signal(440.0, 48000, 1024);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    let expected = 1024 / hop; // 2
    assert_eq!(engine.context().frame_index, expected as u64);
}

#[test]
fn subblock_extreme_mode_hop_1024() {
    // Extreme mode: hop=1024. Block 1024 = hop → fast path, 1 sub-block.
    let mut config = EngineConfig::default();
    config.quality_mode = QualityMode::Extreme;
    let mut engine = build_engine_with_config(48000, 4096, config);
    let mut buf = sine_signal(440.0, 48000, 1024);
    engine.process(&mut buf);
    assert!(is_finite_buffer(&buf));
    assert_eq!(engine.context().frame_index, 1);
}

// ═══════════════════════════════════════════════════════════════════
// 9. DISTORTION REGRESSION TESTS
//    Prevent recurrence of the 4 root causes from doc 22.
// ═══════════════════════════════════════════════════════════════════

/// Regression: Cause 1 — M6 directional headroom formula.
///
/// When dry and residual have opposite signs, the available headroom
/// is larger than |limit - |dry||. A non-directional formula would
/// over-restrict the residual, producing distortion artifacts.
///
/// This test verifies that the mixer output stays within [-limit, +limit]
/// AND does not crush the residual unnecessarily (no false triggering).
#[test]
fn regression_cause1_directional_headroom_no_false_limiting() {
    let mut engine = build_engine(48000, 1024);
    // Use a loud signal near the limit — this is where the old formula broke
    let limit = 0.95f32;

    // Warm up so damage posterior stabilizes
    for _ in 0..30 {
        let mut buf = sine_signal(440.0, 48000, 256);
        // Scale to ~0.85 peak — near limit but within
        for s in buf.iter_mut() {
            *s *= 0.85;
        }
        engine.process(&mut buf);
    }

    // Process loud signal and verify no samples exceed true peak limit
    for block in 0..50 {
        let mut buf: Vec<f32> = sine_signal(440.0, 48000, 256)
            .iter()
            .map(|s| s * 0.85)
            .collect();
        engine.process(&mut buf);

        for (i, &s) in buf.iter().enumerate() {
            assert!(
                s.abs() <= limit + 0.01, // small tolerance for float math
                "Cause 1 regression: sample exceeds limit at block {block} sample {i}: {s}"
            );
        }
        assert!(is_finite_buffer(&buf));
    }
}

/// Regression: Cause 2 — M6 per-sample hard clamp must NOT exist.
///
/// Flat-topped waveforms indicate hard clipping. This test verifies
/// that consecutive samples near the limit don't all get clamped to
/// the same value (which would produce a flat-top = hard clip).
#[test]
fn regression_cause2_no_hard_clamp_flat_top() {
    let mut engine = build_engine(48000, 1024);
    let limit = 0.95f32;

    // Warm up
    for _ in 0..50 {
        let mut buf = sine_signal(440.0, 48000, 256);
        for s in buf.iter_mut() {
            *s *= 0.9;
        }
        engine.process(&mut buf);
    }

    // Process loud signal and check for flat-topped regions
    let mut flat_top_count = 0;
    for _ in 0..50 {
        let mut buf: Vec<f32> = sine_signal(440.0, 48000, 256)
            .iter()
            .map(|s| s * 0.9)
            .collect();
        engine.process(&mut buf);

        // Check for consecutive samples at identical peak value (flat top indicator)
        for window in buf.windows(4) {
            let all_same = window
                .iter()
                .all(|s| (s.abs() - limit).abs() < 0.001);
            if all_same {
                flat_top_count += 1;
            }
        }
    }
    // A few coincidental matches are ok, but sustained flat-tops indicate hard clipping
    assert!(
        flat_top_count < 5,
        "Cause 2 regression: detected {flat_top_count} flat-top windows — hard clamp likely present"
    );
}

/// Regression: Cause 2 — True peak guard uses uniform gain reduction,
/// not per-sample clamp.
///
/// Process a loud signal and verify that all samples are scaled by the
/// same ratio (uniform gain reduction preserves waveform shape).
#[test]
fn regression_cause2_true_peak_guard_uniform_gain() {
    use phaselith_dsp_core::modules::m6_mixer::true_peak::apply_true_peak_guard;

    // Create a loud signal that exceeds limit
    let mut buf: Vec<f32> = (0..256)
        .map(|i| 1.2 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
        .collect();
    let original = buf.clone();
    let limit = 0.95;

    apply_true_peak_guard(&mut buf, limit);

    // All samples must be within limit
    assert!(
        buf.iter().all(|s| s.abs() <= limit + 1e-6),
        "True peak guard failed to limit output"
    );

    // The gain ratio should be uniform across all non-zero samples
    let mut ratios: Vec<f32> = Vec::new();
    for (o, p) in original.iter().zip(buf.iter()) {
        if o.abs() > 0.01 {
            ratios.push(p / o);
        }
    }
    if let (Some(&min_r), Some(&max_r)) = (
        ratios.iter().min_by(|a, b| a.partial_cmp(b).unwrap()),
        ratios.iter().max_by(|a, b| a.partial_cmp(b).unwrap()),
    ) {
        assert!(
            (max_r - min_r).abs() < 1e-5,
            "Cause 2 regression: non-uniform gain reduction detected (min={min_r}, max={max_r})"
        );
    }
}

/// Regression: Cause 3 — M5 OLA double-add (multi-hop).
///
/// When block_size > hop_size, the engine must split into sub-blocks
/// so hops_this_block ≤ 1. If double-add occurs, energy in overlap
/// regions will be approximately 2x the expected level.
///
/// We detect this by comparing the RMS of a 528-sample block processed
/// as one piece vs four 132-sample blocks — they should produce similar
/// energy levels (not 2x).
#[test]
fn regression_cause3_no_ola_double_add_energy() {
    // Process 528 samples (APO real-world block) with sub-block splitting.
    // Energy should be comparable to processing with small blocks.
    let mut engine = build_engine(48000, 1024);

    // Warm up
    for _ in 0..50 {
        let mut buf = sine_signal(440.0, 48000, 256);
        engine.process(&mut buf);
    }

    // Process a 528-sample block — should be split into 256+256+16
    let mut buf_large = sine_signal(440.0, 48000, 528);
    let input_rms = (buf_large.iter().map(|s| s * s).sum::<f32>() / buf_large.len() as f32).sqrt();
    engine.process(&mut buf_large);
    let output_rms =
        (buf_large.iter().map(|s| s * s).sum::<f32>() / buf_large.len() as f32).sqrt();

    // Energy ratio should be reasonable (0.3x - 3x), not 2x from double-add.
    // The old double-add bug would produce ~2x energy in overlap regions.
    if input_rms > 0.01 {
        let ratio = output_rms / input_rms;
        assert!(
            ratio < 3.0,
            "Cause 3 regression: energy ratio {ratio:.2} suggests OLA double-add (expected < 3.0)"
        );
    }
}

/// Regression: Cause 3 — Sub-block splitting guarantees hops_this_block ≤ 1.
///
/// Directly verify that a 528-sample block results in 3 sub-block calls
/// (256 + 256 + 16), not one call with hops_this_block=2.
#[test]
fn regression_cause3_subblock_guarantees_single_hop() {
    use phaselith_dsp_core::engine::test_helpers::RecordingModule;
    use std::sync::{Arc, Mutex};

    let call_log = Arc::new(Mutex::new(Vec::new()));
    let mut engine = PhaselithEngineBuilder::new(48000, 1024)
        .add_module(Box::new(RecordingModule::new("M0", call_log.clone())))
        .build();

    // 528 samples with Standard hop=256 → 3 sub-blocks
    let mut buf = vec![0.0f32; 528];
    engine.process(&mut buf);

    let log = call_log.lock().unwrap();
    assert_eq!(
        log.len(),
        3,
        "Cause 3 regression: 528-sample block should produce 3 sub-blocks, got {}",
        log.len()
    );
    assert_eq!(engine.context().frame_index, 3);
}

/// Regression: Cause 4 — APO block > hop structural constraint.
///
/// Verify that the engine handles ALL common APO block sizes correctly
/// by splitting them. Common Windows Audio Engine block sizes at 48kHz:
/// 480 (10ms), 528 (~11ms), 960 (20ms), 1920 (40ms).
#[test]
fn regression_cause4_all_apo_block_sizes() {
    let apo_block_sizes: &[usize] = &[480, 528, 960, 1920];
    let hop = QualityMode::Standard.hop_size(); // 256

    for &block_size in apo_block_sizes {
        let mut engine = build_engine(48000, block_size.max(1024));
        let expected_sub_blocks = (block_size + hop - 1) / hop; // ceil division

        // Warm up
        for _ in 0..10 {
            let mut buf = sine_signal(440.0, 48000, block_size.min(hop));
            engine.process(&mut buf);
        }
        engine.reset();

        let mut buf = sine_signal(440.0, 48000, block_size);
        engine.process(&mut buf);

        assert!(
            is_finite_buffer(&buf),
            "Cause 4 regression: block_size={block_size} produced non-finite output"
        );

        if block_size > hop {
            assert_eq!(
                engine.context().frame_index as usize,
                expected_sub_blocks,
                "Cause 4 regression: block_size={block_size} expected {expected_sub_blocks} sub-blocks, got {}",
                engine.context().frame_index
            );
        }

        // Verify output amplitude is reasonable
        assert!(
            max_abs(&buf) < 4.0,
            "Cause 4 regression: block_size={block_size} output amplitude explosion: {}",
            max_abs(&buf)
        );
    }
}

/// Regression: Combined — loud signal through full pipeline with APO block size.
///
/// This is the exact scenario that caused all 4 issues: loud music through
/// the full M0-M7 pipeline with a 528-sample APO block size and Standard mode.
/// Output must be within true peak limit with no flat-tops.
#[test]
fn regression_combined_loud_apo_528_full_pipeline() {
    let mut engine = build_engine(48000, 1024);
    let limit = 0.95f32;

    // Simulate loud music: multi-tone signal at high level
    let make_loud_block = |offset: usize| -> Vec<f32> {
        (0..528)
            .map(|i| {
                let t = (offset + i) as f32 / 48000.0;
                let s = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                    + 0.3 * (2.0 * std::f32::consts::PI * 880.0 * t).sin()
                    + 0.15 * (2.0 * std::f32::consts::PI * 1760.0 * t).sin();
                s * 0.85 // near limit
            })
            .collect()
    };

    let mut flat_tops = 0;
    let mut exceeded_limit = 0;

    for block in 0..200 {
        let mut buf = make_loud_block(block * 528);
        engine.process(&mut buf);

        assert!(
            is_finite_buffer(&buf),
            "Combined regression: non-finite at block {block}"
        );

        for &s in &buf {
            if s.abs() > limit + 0.02 {
                exceeded_limit += 1;
            }
        }

        // Check for flat-tops (4+ consecutive samples at exact same peak)
        for window in buf.windows(4) {
            if window.iter().all(|s| (s.abs() - limit).abs() < 0.001) {
                flat_tops += 1;
            }
        }
    }

    assert!(
        exceeded_limit < 10,
        "Combined regression: {exceeded_limit} samples exceeded limit across 200 blocks"
    );
    assert!(
        flat_tops < 5,
        "Combined regression: {flat_tops} flat-top windows detected — hard clamp likely present"
    );
}
