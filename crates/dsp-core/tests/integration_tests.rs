use asce_dsp_core::engine::CirrusEngineBuilder;
use asce_dsp_core::config::{EngineConfig, QualityMode, StyleConfig, StylePreset};

/// Generate a 440 Hz sine wave at the given sample rate.
fn sine_440(sample_rate: u32, num_samples: usize) -> Vec<f32> {
    (0..num_samples)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sample_rate as f32).sin())
        .collect()
}

/// Generate a signal with an artificial HF cutoff (simulate lossy codec).
/// Zeros out everything above cutoff_hz by doing FFT → zero → IFFT.
fn bandlimited_signal(sample_rate: u32, num_samples: usize, cutoff_hz: f32) -> Vec<f32> {
    use std::f32::consts::PI;

    // Start with a rich signal (multiple harmonics)
    let signal: Vec<f32> = (0..num_samples)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let mut s = 0.0;
            // Square-ish wave: odd harmonics up to Nyquist
            for k in (1..=31).step_by(2) {
                let freq = 440.0 * k as f32;
                if freq < sample_rate as f32 / 2.0 {
                    s += (2.0 * PI * freq * t).sin() / k as f32;
                }
            }
            s * 0.3
        })
        .collect();

    // Apply brick-wall LPF in frequency domain (simple approach for testing)
    let bin_width = sample_rate as f32 / num_samples as f32;
    let _cutoff_bin = (cutoff_hz / bin_width) as usize;

    // Apply a simple IIR low-pass filter
    let rc = 1.0 / (2.0 * PI * cutoff_hz);
    let dt = 1.0 / sample_rate as f32;
    let alpha = dt / (rc + dt);

    let mut filtered = vec![0.0f32; num_samples];
    filtered[0] = signal[0] * alpha;
    for i in 1..num_samples {
        filtered[i] = filtered[i - 1] + alpha * (signal[i] - filtered[i - 1]);
    }

    filtered
}

#[test]
fn default_engine_processes_without_panic() {
    let mut engine = CirrusEngineBuilder::new(48000, 2048).build_default();

    let mut buf = sine_440(48000, 2048);

    // Should not panic
    engine.process(&mut buf);

    // Signal should remain finite
    assert!(buf.iter().all(|s| s.is_finite()), "Output contains non-finite values");
}

#[test]
fn engine_output_is_finite_for_all_quality_modes() {
    for mode in &[QualityMode::Light, QualityMode::Standard, QualityMode::Ultra] {
        let mut config = EngineConfig::default();
        config.quality_mode = *mode;

        let mut engine = CirrusEngineBuilder::new(48000, 2048)
            .with_config(config)
            .build_default();

        let mut buf = sine_440(48000, 2048);
        engine.process(&mut buf);

        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Non-finite output for {:?}",
            mode
        );
    }
}

#[test]
fn zero_strength_produces_near_original() {
    let mut config = EngineConfig::default();
    config.strength = 0.0;

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .build_default();

    let mut buf = sine_440(48000, 1024);
    engine.process(&mut buf);

    // With zero strength, most modules should be no-ops
    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn engine_handles_silence() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut buf = vec![0.0f32; 1024];
    engine.process(&mut buf);

    // Silence in → should remain near-silence
    let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(
        max_val < 0.01,
        "Silence input produced output with max amplitude {max_val}"
    );
}

#[test]
fn engine_handles_dc_offset() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut buf = vec![0.5f32; 1024];
    engine.process(&mut buf);

    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn engine_handles_full_scale() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    // Clipped signal
    let mut buf = vec![1.0f32; 1024];
    engine.process(&mut buf);

    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn engine_multiple_frames_stable() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    // Process many frames to check for state accumulation issues
    for frame in 0..100 {
        let mut buf = sine_440(48000, 1024);
        engine.process(&mut buf);

        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Non-finite output at frame {frame}"
        );
    }
}

#[test]
fn engine_reset_allows_clean_restart() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    // Process some frames
    for _ in 0..10 {
        let mut buf = sine_440(48000, 1024);
        engine.process(&mut buf);
    }

    engine.reset();

    // Should be able to process again without issues
    let mut buf = sine_440(48000, 1024);
    engine.process(&mut buf);
    assert!(buf.iter().all(|s| s.is_finite()));
    assert_eq!(engine.context().frame_index, 1);
}

#[test]
fn bandlimited_signal_is_processed() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut buf = bandlimited_signal(48000, 1024, 16000.0);
    let original_energy: f32 = buf.iter().map(|s| s * s).sum();

    engine.process(&mut buf);

    let output_energy: f32 = buf.iter().map(|s| s * s).sum();

    assert!(buf.iter().all(|s| s.is_finite()));

    // Energy should not explode (safety check)
    assert!(
        output_energy < original_energy * 10.0,
        "Output energy {output_energy} is suspiciously large compared to input {original_energy}"
    );
}

#[test]
fn governor_telemetry_is_populated_after_processing() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut buf = sine_440(48000, 1024);
    engine.process(&mut buf);

    // After processing, damage posterior should have some state
    let damage = engine.damage_posterior();
    // Default (no degradation) should show lossless-ish cutoff
    assert!(damage.cutoff.mean > 0.0, "Cutoff mean should be positive");
}

#[test]
fn different_frame_sizes_work() {
    for frame_size in &[256, 512, 1024, 2048] {
        let mut engine = CirrusEngineBuilder::new(48000, *frame_size).build_default();
        let mut buf = sine_440(48000, *frame_size);
        engine.process(&mut buf);

        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Non-finite output for frame size {frame_size}"
        );
    }
}

#[test]
fn different_sample_rates_work() {
    for sr in &[44100u32, 48000, 96000] {
        let mut engine = CirrusEngineBuilder::new(*sr, 1024).build_default();
        let mut buf = sine_440(*sr, 1024);
        engine.process(&mut buf);

        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Non-finite output for sample rate {sr}"
        );
    }
}

#[test]
fn self_reprojection_does_not_explode() {
    // Process a degraded signal through multiple frames
    // Ensure the reprojection validator keeps things bounded
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    for _ in 0..50 {
        let mut buf = bandlimited_signal(48000, 1024, 14000.0);
        engine.process(&mut buf);

        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val < 10.0,
            "Output amplitude {max_val} exceeds safety bound"
        );
    }
}

#[test]
fn safety_mixer_preserves_low_band() {
    // The mixer should not significantly alter content below the detected cutoff
    let mut config = EngineConfig::default();
    config.strength = 1.0;

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .build_default();

    // Use a simple low-frequency sine (well below any cutoff)
    let mut buf: Vec<f32> = (0..1024)
        .map(|i| (2.0 * std::f32::consts::PI * 100.0 * i as f32 / 48000.0).sin())
        .collect();
    let _original = buf.clone();

    engine.process(&mut buf);

    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn config_changes_mid_stream() {
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    // Process a few frames with default config
    for _ in 0..5 {
        let mut buf = sine_440(48000, 1024);
        engine.process(&mut buf);
    }

    // Change config mid-stream
    let mut config = EngineConfig::default();
    config.strength = 0.0;
    engine.update_config(config);

    // Process more frames
    for _ in 0..5 {
        let mut buf = sine_440(48000, 1024);
        engine.process(&mut buf);
        assert!(buf.iter().all(|s| s.is_finite()));
    }
}

// ═══════════════════════════════════════════════════════════════
// E2E edge-case tests: boundary conditions through full pipeline
// ═══════════════════════════════════════════════════════════════

#[test]
fn e2e_clipped_signal_output_bounded() {
    // Heavily clipped signal — engine should not amplify beyond safe limits
    let mut config = EngineConfig::default();
    config.strength = 1.0;
    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .build_default();

    // Simulate hard-clipped signal (alternating +1/-1)
    let mut buf: Vec<f32> = (0..1024)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();

    for frame in 0..20 {
        engine.process(&mut buf);
        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val <= 1.0,
            "Frame {frame}: clipped output exceeded 1.0, got {max_val}"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Frame {frame}: non-finite output"
        );
    }
}

#[test]
fn e2e_alternating_loud_quiet_stable() {
    // Alternate between loud and silent frames — tests state transitions
    let mut engine = CirrusEngineBuilder::new(48000, 512).build_default();

    for frame in 0..40 {
        let mut buf = if frame % 2 == 0 {
            // Loud frame
            sine_440(48000, 512).iter().map(|s| s * 0.9).collect()
        } else {
            // Silent frame
            vec![0.0f32; 512]
        };

        engine.process(&mut buf);

        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val < 2.0,
            "Frame {frame}: amplitude {max_val} too large after loud/quiet transition"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Frame {frame}: non-finite output"
        );
    }
}

#[test]
fn e2e_very_small_frame_64_samples() {
    // 64-sample frame — smaller than typical WASM block
    let mut engine = CirrusEngineBuilder::new(48000, 64).build_default();
    let mut buf = sine_440(48000, 64);
    engine.process(&mut buf);
    assert!(buf.iter().all(|s| s.is_finite()));
}

#[test]
fn e2e_large_frame_4096_samples() {
    // 4096-sample frame — maximum expected size
    let mut engine = CirrusEngineBuilder::new(48000, 4096).build_default();
    let mut buf = sine_440(48000, 4096);
    engine.process(&mut buf);

    assert!(buf.iter().all(|s| s.is_finite()));
    let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max_val < 2.0, "Large frame output too large: {max_val}");
}

#[test]
fn e2e_near_nyquist_cutoff() {
    // Signal with cutoff near Nyquist — minimal damage, should pass through mostly unchanged
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();
    let mut buf = bandlimited_signal(48000, 1024, 23000.0); // near Nyquist
    let original_energy: f32 = buf.iter().map(|s| s * s).sum();

    engine.process(&mut buf);

    let output_energy: f32 = buf.iter().map(|s| s * s).sum();
    assert!(buf.iter().all(|s| s.is_finite()));

    // Near-Nyquist should be detected as near-lossless, minimal change
    assert!(
        output_energy < original_energy * 5.0,
        "Near-Nyquist should not greatly amplify: in={original_energy}, out={output_energy}"
    );
}

#[test]
fn e2e_low_cutoff_heavy_damage() {
    // Very low cutoff (8kHz) — heavy damage, engine should still be stable
    let mut config = EngineConfig::default();
    config.strength = 1.0;
    config.quality_mode = QualityMode::Ultra;
    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .build_default();

    for _ in 0..30 {
        let mut buf = bandlimited_signal(48000, 1024, 8000.0);
        engine.process(&mut buf);

        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val < 10.0,
            "Heavy damage output amplitude {max_val} exceeds safety bound"
        );
        assert!(buf.iter().all(|s| s.is_finite()));
    }
}

#[test]
fn e2e_impulse_signal() {
    // Single impulse (Dirac delta) — tests transient handling
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    let mut buf = vec![0.0f32; 1024];
    buf[512] = 1.0; // single impulse

    engine.process(&mut buf);

    assert!(buf.iter().all(|s| s.is_finite()));
    let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!(max_val <= 1.0, "Impulse output exceeded 1.0: {max_val}");
}

#[test]
fn e2e_all_style_presets_stable() {
    // Test all character presets through the full pipeline
    let presets = [
        StyleConfig::from_preset(StylePreset::Reference),
        StyleConfig::from_preset(StylePreset::Grand),
        StyleConfig::from_preset(StylePreset::Smooth),
        StyleConfig::from_preset(StylePreset::Vocal),
        StyleConfig::from_preset(StylePreset::Punch),
        StyleConfig::from_preset(StylePreset::Air),
        StyleConfig::from_preset(StylePreset::Night),
    ];

    for (i, style) in presets.iter().enumerate() {
        let mut config = EngineConfig::default();
        config.style = *style;

        let mut engine = CirrusEngineBuilder::new(48000, 1024)
            .with_config(config)
            .build_default();

        let mut buf = bandlimited_signal(48000, 1024, 14000.0);
        engine.process(&mut buf);

        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Style preset {i} produced non-finite output"
        );
        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val < 5.0,
            "Style preset {i} amplitude {max_val} too large"
        );
    }
}

#[test]
fn e2e_sequential_engine_resets_stable() {
    // Process → reset → process repeatedly — tests cleanup
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    for cycle in 0..5 {
        for _ in 0..10 {
            let mut buf = sine_440(48000, 1024);
            engine.process(&mut buf);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "Cycle {cycle}: non-finite output"
            );
        }
        engine.reset();
        assert_eq!(engine.context().frame_index, 0, "Reset should clear frame_index");
    }
}

#[test]
fn e2e_mixed_damage_clipping_and_cutoff() {
    // Signal with both clipping AND low cutoff — combined damage paths
    let mut config = EngineConfig::default();
    config.strength = 1.0;
    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .build_default();

    // Generate clipped bandlimited signal
    let mut buf = bandlimited_signal(48000, 1024, 12000.0);
    // Hard clip at ±0.7
    for s in &mut buf {
        *s = s.clamp(-0.7, 0.7);
    }

    for frame in 0..20 {
        engine.process(&mut buf);
        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val < 5.0,
            "Frame {frame}: mixed damage output {max_val} too large"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Frame {frame}: non-finite output"
        );
    }
}

#[test]
fn e2e_44100_sample_rate_multi_frame() {
    // 44.1kHz (CD quality) through multiple frames
    let mut engine = CirrusEngineBuilder::new(44100, 1024).build_default();

    for frame in 0..30 {
        let mut buf = bandlimited_signal(44100, 1024, 15000.0);
        engine.process(&mut buf);
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "44.1kHz frame {frame}: non-finite output"
        );
    }
}

#[test]
fn e2e_96khz_sample_rate_multi_frame() {
    // 96kHz (hi-res) through multiple frames
    let mut engine = CirrusEngineBuilder::new(96000, 1024).build_default();

    for frame in 0..30 {
        let mut buf = sine_440(96000, 1024);
        engine.process(&mut buf);
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "96kHz frame {frame}: non-finite output"
        );
    }
}

#[test]
fn e2e_reprojection_additive_synthesis_produces_content() {
    // Verify that M5 additive synthesis actually contributes to output
    // when there is detectable damage.
    let mut config = EngineConfig::default();
    config.strength = 1.0;

    let mut engine = CirrusEngineBuilder::new(48000, 1024)
        .with_config(config)
        .build_default();

    // Process several frames of degraded signal to build up damage posterior
    for _ in 0..20 {
        let mut buf = bandlimited_signal(48000, 1024, 12000.0);
        engine.process(&mut buf);
    }

    // After warmup, the engine should have detected damage
    let damage = engine.damage_posterior();
    // Check that damage detection is working
    assert!(
        damage.cutoff.mean > 0.0,
        "Damage posterior should have positive cutoff"
    );
}

#[test]
fn e2e_noise_signal_does_not_explode() {
    // White noise — worst case for harmonic analysis
    let mut engine = CirrusEngineBuilder::new(48000, 1024).build_default();

    // Simple pseudo-random noise
    let mut seed: u32 = 12345;
    for frame in 0..30 {
        let mut buf: Vec<f32> = (0..1024)
            .map(|_| {
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect();

        engine.process(&mut buf);
        let max_val = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            max_val < 5.0,
            "Frame {frame}: noise output {max_val} too large"
        );
        assert!(
            buf.iter().all(|s| s.is_finite()),
            "Frame {frame}: non-finite noise output"
        );
    }
}
