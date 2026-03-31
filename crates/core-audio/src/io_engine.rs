// Platform-independent audio processing engine.
//
// Dual-engine architecture identical to APO (apo_impl.rs):
// - Two independent mono PhaselithEngine instances (L/R)
// - Symmetric one-frame-delayed cross-channel context
// - Pre-allocated scratch buffers (zero-alloc on real-time thread)
// - NaN/Inf guard, warmup blend, enable/disable crossfade
//
// This file has NO OS dependencies — compiles and tests on all platforms.

use crate::mmap_ipc::{SharedConfig, SharedStatus};
use phaselith_dsp_core::config::{EngineConfig, PhaseMode, QualityMode, StyleConfig, SynthesisMode};
use phaselith_dsp_core::engine::{PhaselithEngine, PhaselithEngineBuilder};
use phaselith_dsp_core::types::CrossChannelContext;
use std::sync::atomic::Ordering;

/// Platform-independent audio processing engine.
/// Used by both CoreAudio HAL plugin and potentially other platform adapters.
pub struct IoEngine {
    engine_l: Option<PhaselithEngine>,
    engine_r: Option<PhaselithEngine>,
    sample_rate: u32,
    channels: u16,
    frame_size: usize,
    active: bool,
    bypass_mode: bool,
    channel_buf_l: Vec<f32>,
    channel_buf_r: Vec<f32>,
    dry_lr_saved: Vec<f32>,
    cross_channel_prev: Option<CrossChannelContext>,
    last_config_version: u32,
    prev_enabled: bool,
    process_frame_count: u64,
    shutting_down: bool,
}

impl IoEngine {
    const WARMUP_FRAMES: u64 = 16;
    const PRIME_FRAMES: usize = 6;

    pub fn new() -> Self {
        Self {
            engine_l: None,
            engine_r: None,
            sample_rate: 48000,
            channels: 2,
            frame_size: 480,
            active: false,
            bypass_mode: false,
            channel_buf_l: Vec::new(),
            channel_buf_r: Vec::new(),
            dry_lr_saved: Vec::new(),
            cross_channel_prev: None,
            last_config_version: 0,
            prev_enabled: false,
            process_frame_count: 0,
            shutting_down: false,
        }
    }

    /// Store format parameters. Called during plugin initialization (non-RT).
    pub fn initialize(&mut self, sample_rate: u32, channels: u16) {
        self.sample_rate = sample_rate;
        self.channels = channels;
    }

    /// Build engines, allocate scratch buffers, prime with silence.
    /// Equivalent to APO's lock_for_process(). Called before IO starts (non-RT).
    pub fn start(&mut self, frame_size: usize) {
        self.frame_size = frame_size;
        let config = EngineConfig::default();

        self.engine_l = Some(
            PhaselithEngineBuilder::new(self.sample_rate, frame_size)
                .with_config(config)
                .with_channels(1)
                .build_default(),
        );

        if self.channels >= 2 {
            self.engine_r = Some(
                PhaselithEngineBuilder::new(self.sample_rate, frame_size)
                    .with_config(config)
                    .with_channels(1)
                    .build_default(),
            );
        }

        self.channel_buf_l = vec![0.0f32; frame_size];
        self.channel_buf_r = vec![0.0f32; frame_size];
        self.dry_lr_saved = vec![0.0f32; frame_size * 2];
        self.cross_channel_prev = None;
        self.last_config_version = 0;
        self.process_frame_count = 0;
        self.shutting_down = false;

        // Prime engines with silent frames to fill OLA buffer
        {
            let mut silent = vec![0.0f32; frame_size];
            for _ in 0..Self::PRIME_FRAMES {
                if let Some(ref mut engine) = self.engine_l {
                    engine.process(&mut silent);
                }
                silent.fill(0.0);
            }
            if self.channels >= 2 {
                let mut silent_r = vec![0.0f32; frame_size];
                for _ in 0..Self::PRIME_FRAMES {
                    if let Some(ref mut engine) = self.engine_r {
                        engine.process(&mut silent_r);
                    }
                    silent_r.fill(0.0);
                }
            }
        }

        self.active = true;
    }

    /// Release engines. Equivalent to APO's unlock_for_process().
    pub fn stop(&mut self) {
        self.shutting_down = true;
        self.active = false;
        self.engine_l = None;
        self.engine_r = None;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_bypass_mode(&mut self) {
        self.bypass_mode = true;
        self.engine_l = None;
        self.engine_r = None;
        self.active = true; // bypass still "active" — just passes through
    }

    /// Hot-reload config from shared memory. Called from RT thread.
    /// Only reads atomics — no alloc, no lock, no I/O.
    pub fn update_config_from_shared(&mut self, config: &SharedConfig) {
        let version = config.version.load(Ordering::Acquire);
        if version == self.last_config_version {
            return;
        }
        self.last_config_version = version;

        let engine_config = Self::shared_config_to_engine_config(config);
        if let Some(ref mut engine) = self.engine_l {
            engine.update_config(engine_config);
        }
        if let Some(ref mut engine) = self.engine_r {
            engine.update_config(engine_config);
        }
    }

    /// Write processing status to shared memory. Called from RT thread.
    pub fn write_status(&self, status: &SharedStatus, input: &[f32], output: &[f32], enabled: bool) {
        status.increment_frames();

        if let Some(ref engine) = self.engine_l {
            let damage = engine.damage_posterior();
            status.set_cutoff(Some(damage.cutoff.mean));
            status.set_clipping(damage.clipping.mean);

            let frames = input.len() / self.channels.max(1) as usize;
            let buffer_duration_us = (frames as f32 / self.sample_rate as f32) * 1_000_000.0;
            let load_percent = if buffer_duration_us > 0.0 {
                (engine.context().processing_time_us / buffer_duration_us) * 100.0
            } else {
                0.0
            };
            status.set_processing_load(load_percent);
        }

        if enabled && !self.shutting_down {
            let n = input.len().min(output.len());
            let mut diff_sum = 0.0f32;
            for i in 0..n {
                let d = output[i] - input[i];
                diff_sum += d * d;
            }
            let rms = (diff_sum / n.max(1) as f32).sqrt();
            let db = if rms > 1e-10 { 20.0 * rms.log10() } else { -120.0 };
            status.set_wet_dry_diff_db(db);
        } else {
            status.set_wet_dry_diff_db(-120.0);
        }
    }

    /// Real-time audio processing. MUST be deterministic:
    /// - No allocation
    /// - No mutex/lock
    /// - No I/O
    /// - No panic (caller wraps in catch_unwind)
    pub fn process(&mut self, input: &[f32], output: &mut [f32], enabled: bool) {
        let copy_len = input.len().min(output.len());
        if copy_len == 0 { return; }

        if !self.active || self.bypass_mode {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }

        if self.shutting_down {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }

        let ch = self.channels as usize;
        if ch == 0 {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }
        let frames = input.len() / ch;

        // Validate scratch buffers
        if frames > self.channel_buf_l.len() || frames > self.channel_buf_r.len()
            || frames * 2 > self.dry_lr_saved.len()
        {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }

        let transitioning = enabled != self.prev_enabled;
        self.prev_enabled = enabled;
        self.process_frame_count += 1;
        let warmup = self.warmup_blend();

        if ch == 1 {
            self.process_mono(input, output, frames, enabled, transitioning, warmup);
        } else if ch >= 2 {
            self.process_stereo(input, output, frames, copy_len, enabled, transitioning, warmup);
        }
    }

    fn process_mono(
        &mut self, input: &[f32], output: &mut [f32], frames: usize,
        enabled: bool, transitioning: bool, warmup: f32,
    ) {
        if let Some(ref mut engine) = self.engine_l {
            self.channel_buf_l[..frames].copy_from_slice(&input[..frames]);
            engine.process(&mut self.channel_buf_l[..frames]);

            let clean = Self::is_buffer_clean(&self.channel_buf_l, frames);
            if !clean {
                output[..frames].copy_from_slice(&input[..frames]);
            } else if transitioning {
                for f in 0..frames {
                    let t = (f as f32 + 1.0) / frames as f32;
                    let (from, to) = if enabled {
                        (input[f], self.channel_buf_l[f])
                    } else {
                        (self.channel_buf_l[f], input[f])
                    };
                    output[f] = from * (1.0 - t) + to * t;
                }
            } else if enabled {
                if warmup >= 1.0 {
                    output[..frames].copy_from_slice(&self.channel_buf_l[..frames]);
                } else {
                    for f in 0..frames {
                        output[f] = input[f] * (1.0 - warmup) + self.channel_buf_l[f] * warmup;
                    }
                }
            } else {
                output[..frames].copy_from_slice(&input[..frames]);
            }
        } else {
            output[..frames].copy_from_slice(&input[..frames]);
        }
    }

    fn process_stereo(
        &mut self, input: &[f32], output: &mut [f32], frames: usize, copy_len: usize,
        enabled: bool, transitioning: bool, warmup: f32,
    ) {
        if input.len() < frames * 2 || output.len() < frames * 2 {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }

        // De-interleave and save dry copies
        for f in 0..frames {
            let l = input[f * 2];
            let r = input[f * 2 + 1];
            self.channel_buf_l[f] = l;
            self.channel_buf_r[f] = r;
            self.dry_lr_saved[f] = l;
            self.dry_lr_saved[frames + f] = r;
        }

        // Process L
        if let Some(ref mut engine) = self.engine_l {
            engine.context_mut().cross_channel = self.cross_channel_prev;
            engine.process(&mut self.channel_buf_l[..frames]);
        }

        // Process R
        if let Some(ref mut engine) = self.engine_r {
            engine.context_mut().cross_channel = self.cross_channel_prev;
            engine.process(&mut self.channel_buf_r[..frames]);
        }

        // Update cross-channel context (one-frame delay)
        let cc = CrossChannelContext::from_lr(
            &self.dry_lr_saved[..frames],
            &self.dry_lr_saved[frames..frames * 2],
        );
        self.cross_channel_prev = Some(cc);

        // NaN/Inf guard
        let clean_l = Self::is_buffer_clean(&self.channel_buf_l, frames);
        let clean_r = Self::is_buffer_clean(&self.channel_buf_r, frames);
        if !clean_l || !clean_r {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
        } else if transitioning {
            for f in 0..frames {
                let t = (f as f32 + 1.0) / frames as f32;
                if enabled {
                    output[f * 2] = self.dry_lr_saved[f] * (1.0 - t) + self.channel_buf_l[f] * t;
                    output[f * 2 + 1] = self.dry_lr_saved[frames + f] * (1.0 - t) + self.channel_buf_r[f] * t;
                } else {
                    output[f * 2] = self.channel_buf_l[f] * (1.0 - t) + self.dry_lr_saved[f] * t;
                    output[f * 2 + 1] = self.channel_buf_r[f] * (1.0 - t) + self.dry_lr_saved[frames + f] * t;
                }
            }
        } else if enabled {
            if warmup >= 1.0 {
                for f in 0..frames {
                    output[f * 2] = self.channel_buf_l[f];
                    output[f * 2 + 1] = self.channel_buf_r[f];
                }
            } else {
                for f in 0..frames {
                    output[f * 2] = self.dry_lr_saved[f] * (1.0 - warmup) + self.channel_buf_l[f] * warmup;
                    output[f * 2 + 1] = self.dry_lr_saved[frames + f] * (1.0 - warmup) + self.channel_buf_r[f] * warmup;
                }
            }
        } else {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
        }
    }

    #[inline]
    fn is_buffer_clean(buf: &[f32], len: usize) -> bool {
        for i in 0..len {
            if !buf[i].is_finite() {
                return false;
            }
        }
        true
    }

    #[inline]
    fn warmup_blend(&self) -> f32 {
        if self.process_frame_count >= Self::WARMUP_FRAMES {
            1.0
        } else {
            let t = self.process_frame_count as f32 / Self::WARMUP_FRAMES as f32;
            t * t * (3.0 - 2.0 * t) // smoothstep
        }
    }

    fn shared_config_to_engine_config(sc: &SharedConfig) -> EngineConfig {
        let fs_val = sc.filter_style.load(Ordering::Relaxed) as u32;
        let fs = phaselith_dsp_core::config::FilterStyle::from_u32(fs_val);
        let style = if fs.is_preset() {
            fs.to_style_config()
        } else {
            StyleConfig::new(
                sc.warmth(), sc.air_brightness(), sc.smoothness(),
                sc.spatial_spread(), sc.impact_gain(), sc.body(),
            )
        };

        EngineConfig {
            enabled: true, // Engine always processes; we handle wet/dry externally
            strength: sc.compensation_strength(),
            hf_reconstruction: sc.hf_reconstruction(),
            dynamics: sc.dynamics_restoration(),
            transient: sc.transient_repair(),
            pre_echo_transient_scaling: 0.4,
            declip_transient_scaling: 1.0,
            delayed_transient_repair: false,
            body_pass_enabled: false,
            phase_mode: PhaseMode::Linear,
            quality_mode: QualityMode::Standard,
            style,
            synthesis_mode: SynthesisMode::LegacyAdditive,
            ambience_preserve: 0.0,
            filter_style: fs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_not_started() {
        let mut engine = IoEngine::new();
        let input = vec![0.5f32; 960]; // stereo 480 frames
        let mut output = vec![0.0f32; 960];
        engine.process(&input, &mut output, true);
        assert_eq!(output, input);
    }

    #[test]
    fn passthrough_when_disabled() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.start(480);
        let input = vec![0.1f32; 960];
        let mut output = vec![0.0f32; 960];
        engine.process(&input, &mut output, false);
        // When disabled, output should be dry input
        assert_eq!(output, input);
    }

    #[test]
    fn process_produces_output() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.start(480);

        // Process enough frames to get past warmup
        let input = vec![0.3f32; 960];
        let mut output = vec![0.0f32; 960];
        for _ in 0..20 {
            engine.process(&input, &mut output, true);
        }

        // After warmup, output should differ from input (engine is processing)
        let differs = output.iter().zip(input.iter()).any(|(o, i)| (o - i).abs() > 1e-6);
        assert!(differs, "Engine should modify audio when enabled");
    }

    #[test]
    fn output_is_finite() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.start(480);

        let input = vec![0.5f32; 960];
        let mut output = vec![0.0f32; 960];
        for _ in 0..30 {
            engine.process(&input, &mut output, true);
            for s in output.iter() {
                assert!(s.is_finite(), "Output must not contain NaN/Inf");
            }
        }
    }

    #[test]
    fn mono_mode() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 1);
        engine.start(480);

        let input = vec![0.2f32; 480];
        let mut output = vec![0.0f32; 480];
        engine.process(&input, &mut output, false);
        assert_eq!(output, input);
    }

    #[test]
    fn stop_cleans_up() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.start(480);
        assert!(engine.is_active());

        engine.stop();
        assert!(!engine.is_active());

        // After stop, passthrough
        let input = vec![0.1f32; 960];
        let mut output = vec![0.0f32; 960];
        engine.process(&input, &mut output, true);
        assert_eq!(output, input);
    }

    #[test]
    fn warmup_blend_values() {
        let engine = IoEngine::new();
        // Frame 0: blend = 0
        assert_eq!(engine.warmup_blend(), 0.0);
    }

    #[test]
    fn bypass_mode_passthrough() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.set_bypass_mode();

        let input = vec![0.7f32; 960];
        let mut output = vec![0.0f32; 960];
        engine.process(&input, &mut output, true);
        assert_eq!(output, input);
    }

    #[test]
    fn empty_buffers_no_crash() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.start(480);

        let input: Vec<f32> = vec![];
        let mut output: Vec<f32> = vec![];
        engine.process(&input, &mut output, true);
        // Should not crash
    }

    #[test]
    fn oversized_buffer_passthrough() {
        let mut engine = IoEngine::new();
        engine.initialize(48000, 2);
        engine.start(480);

        // Buffer larger than frame_size → passthrough
        let input = vec![0.5f32; 2000];
        let mut output = vec![0.0f32; 2000];
        engine.process(&input, &mut output, true);
        assert_eq!(output, input);
    }
}
