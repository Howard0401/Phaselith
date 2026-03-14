// ASCE Audio Processing Object implementation.
//
// Implements the COM interfaces required by Windows Audio Engine:
// - IAudioProcessingObject: initialization, format negotiation
// - IAudioProcessingObjectRT: real-time audio processing (APOProcess)
// - IAudioProcessingObjectConfiguration: lock/unlock for process
//
// The APO is loaded by audiodg.exe and processes audio in real-time.
// All processing in APOProcess must be deterministic: no alloc, no locks, no I/O.

use crate::format_negotiate;
use crate::mmap_ipc::MmapIpc;
use asce_dsp_core::config::{EngineConfig, PhaseMode, QualityMode, StyleConfig, SynthesisMode};
use asce_dsp_core::engine::{CirrusEngine, CirrusEngineBuilder};
use asce_dsp_core::types::CrossChannelContext;

/// The ASCE APO instance.
/// Created by ClassFactory, one per audio stream.
///
/// Dual-engine architecture (aligned with browser WASM bridge):
/// - Two independent mono engines (L/R) with no state contamination
/// - Symmetric one-frame-delayed cross-channel context
/// - Each engine sees the same CrossChannelContext from the previous frame
pub struct AsceApo {
    engine_l: Option<CirrusEngine>,
    engine_r: Option<CirrusEngine>,
    mmap: Option<MmapIpc>,
    sample_rate: u32,
    channels: u16,
    frame_size: usize,
    locked: bool,
    /// Pre-allocated scratch buffer for de-interleaved L channel data.
    channel_buf_l: Vec<f32>,
    /// Pre-allocated scratch buffer for de-interleaved R channel data.
    channel_buf_r: Vec<f32>,
    /// Saved dry input for cross-channel computation (pre-allocated).
    /// [0..frame_size] = L dry, [frame_size..2*frame_size] = R dry.
    dry_lr_saved: Vec<f32>,
    /// Cross-channel context from previous frame (symmetric one-frame delay).
    cross_channel_prev: Option<CrossChannelContext>,
    /// Last seen config version from mmap (for change-only updates).
    last_config_version: u32,
}

impl AsceApo {
    pub fn new() -> Self {
        Self {
            engine_l: None,
            engine_r: None,
            mmap: None,
            sample_rate: 48000,
            channels: 2,
            frame_size: 480, // 10ms at 48kHz
            locked: false,
            channel_buf_l: Vec::new(),
            channel_buf_r: Vec::new(),
            dry_lr_saved: Vec::new(),
            cross_channel_prev: None,
            last_config_version: 0,
        }
    }

    /// Called during APO initialization (non-real-time)
    pub fn initialize(&mut self, sample_rate: u32, channels: u16) {
        self.sample_rate = sample_rate;
        self.channels = channels;

        // Try to open mmap IPC (non-fatal if Tauri app isn't running yet)
        self.mmap = MmapIpc::open_or_create().ok();
    }

    /// Check if format is supported
    pub fn is_input_format_supported(
        &self,
        sample_rate: u32,
        bits_per_sample: u16,
        channels: u16,
        is_float: bool,
    ) -> bool {
        format_negotiate::is_format_supported(sample_rate, bits_per_sample, channels, is_float)
    }

    /// Pre-allocate all buffers. Called before processing starts.
    pub fn lock_for_process(&mut self, frame_size: usize) {
        self.frame_size = frame_size;

        // Load config from mmap, or use defaults
        let config = self.load_config();
        let fft_size = config.quality_mode.core_fft_size();

        // Dual-engine: each engine processes one mono channel independently.
        // Aligned with browser WASM bridge architecture (with_channels(1)).
        self.engine_l = Some(
            CirrusEngineBuilder::new(self.sample_rate, fft_size)
                .with_config(config)
                .with_channels(1)
                .build_default(),
        );

        if self.channels >= 2 {
            self.engine_r = Some(
                CirrusEngineBuilder::new(self.sample_rate, fft_size)
                    .with_config(config)
                    .with_channels(1)
                    .build_default(),
            );
        }

        // Pre-allocate scratch buffers (no allocation in hot path)
        self.channel_buf_l = vec![0.0f32; frame_size];
        self.channel_buf_r = vec![0.0f32; frame_size];
        self.dry_lr_saved = vec![0.0f32; frame_size * 2]; // L dry + R dry
        self.cross_channel_prev = None;
        self.last_config_version = 0;

        self.locked = true;
    }

    /// Release resources
    pub fn unlock_for_process(&mut self) {
        self.locked = false;
        self.engine_l = None;
        self.engine_r = None;
    }

    /// Real-time audio processing. MUST be deterministic:
    /// - No allocation
    /// - No mutex/lock
    /// - No I/O
    /// - No panic (wrapped in catch_unwind by caller)
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        if !self.locked {
            output[..input.len()].copy_from_slice(input);
            return;
        }

        // Check bypass via mmap
        if let Some(ref mmap) = self.mmap {
            if !mmap.config().is_enabled() {
                output[..input.len()].copy_from_slice(input);
                return;
            }

            // Hot-reload config only on version change (atomic read, no lock)
            let current_version = mmap.config().version.load(
                std::sync::atomic::Ordering::Relaxed,
            );
            if current_version != self.last_config_version {
                self.last_config_version = current_version;
                let config = self.load_config();
                if let Some(ref mut e) = self.engine_l {
                    e.update_config(config);
                }
                if let Some(ref mut e) = self.engine_r {
                    e.update_config(config);
                }
            }
        }

        let ch = self.channels as usize;
        let frames = input.len() / ch;

        if ch == 1 {
            // Mono: use L engine only
            if let Some(ref mut engine) = self.engine_l {
                output[..frames].copy_from_slice(&input[..frames]);
                engine.process(&mut output[..frames]);
            } else {
                output[..frames].copy_from_slice(&input[..frames]);
            }
        } else {
            // Stereo: dual-engine with symmetric cross-channel context.
            // Aligned with WASM bridge process_block_ch() pattern.

            // De-interleave into pre-allocated buffers and save dry copies
            for f in 0..frames {
                let l = input[f * 2];
                let r = input[f * 2 + 1];
                self.channel_buf_l[f] = l;
                self.channel_buf_r[f] = r;
                self.dry_lr_saved[f] = l;               // L dry
                self.dry_lr_saved[frames + f] = r;       // R dry
            }

            // Inject previous frame's cross-channel context into L engine
            if let Some(ref mut engine) = self.engine_l {
                engine.context_mut().cross_channel = self.cross_channel_prev;
                engine.process(&mut self.channel_buf_l[..frames]);
            }

            // Inject same cross-channel context into R engine (symmetric)
            if let Some(ref mut engine) = self.engine_r {
                engine.context_mut().cross_channel = self.cross_channel_prev;
                engine.process(&mut self.channel_buf_r[..frames]);
            }

            // Compute cross-channel from saved dry L + dry R (AFTER processing)
            // Not used until next frame — symmetric one-frame delay
            let cc = CrossChannelContext::from_lr(
                &self.dry_lr_saved[..frames],
                &self.dry_lr_saved[frames..frames * 2],
            );
            self.cross_channel_prev = Some(cc);

            // Re-interleave output
            for f in 0..frames {
                output[f * 2] = self.channel_buf_l[f];
                output[f * 2 + 1] = self.channel_buf_r[f];
            }
        }

        // Update status via mmap (separate borrow scope to avoid conflicts)
        if let Some(ref mmap) = self.mmap {
            let status = mmap.status();
            status.increment_frames();
            if let Some(ref engine) = self.engine_l {
                let damage = engine.damage_posterior();
                status.set_cutoff(Some(damage.cutoff.mean));
                status.set_clipping(damage.clipping.mean);
                // Processing load: engine processing time / buffer duration
                let buffer_duration_us = (frames as f32 / self.sample_rate as f32) * 1_000_000.0;
                let load_percent = if buffer_duration_us > 0.0 {
                    (engine.context().processing_time_us / buffer_duration_us) * 100.0
                } else {
                    0.0
                };
                status.set_processing_load(load_percent);
            }
        }
    }

    fn load_config(&self) -> EngineConfig {
        if let Some(ref mmap) = self.mmap {
            let sc = mmap.config();
            EngineConfig {
                enabled: sc.is_enabled(),
                strength: sc.compensation_strength(),
                hf_reconstruction: sc.hf_reconstruction(),
                dynamics: sc.dynamics_restoration(),
                transient: sc.transient_repair(),
                phase_mode: match sc.phase_mode.load(std::sync::atomic::Ordering::Relaxed) {
                    1 => PhaseMode::Minimum,
                    _ => PhaseMode::Linear,
                },
                quality_mode: match sc.quality_preset.load(std::sync::atomic::Ordering::Relaxed) {
                    0 => QualityMode::Light,
                    2 => QualityMode::Ultra,
                    _ => QualityMode::Standard,
                },
                style: StyleConfig::default(),
                synthesis_mode: match sc.synthesis_mode.load(std::sync::atomic::Ordering::Relaxed) {
                    0 => SynthesisMode::LegacyAdditive,
                    2 => SynthesisMode::FftOlaFull,
                    _ => SynthesisMode::FftOlaPilot, // default to Pilot (current best)
                },
                ambience_preserve: 0.0,
            }
        } else {
            EngineConfig::default()
        }
    }
}
