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
use asce_dsp_core::config::{EngineConfig, PhaseMode, QualityMode, StyleConfig};
use asce_dsp_core::engine::{CirrusEngine, CirrusEngineBuilder};

/// The ASCE APO instance.
/// Created by ClassFactory, one per audio stream.
///
/// TODO: APO is currently a single-engine sequential mono path with reset between channels.
/// This prevents L→R state contamination but wastes work on reset.
/// Future: redesign as stereo-native reference runtime (dual-engine with shared
/// cross-channel analysis, or true interleaved stereo processing).
pub struct AsceApo {
    engine: Option<CirrusEngine>,
    mmap: Option<MmapIpc>,
    sample_rate: u32,
    channels: u16,
    frame_size: usize,
    locked: bool,
    /// Pre-allocated scratch buffer for de-interleaved channel data.
    /// Sized in lock_for_process() to avoid allocation in process().
    channel_buf: Vec<f32>,
}

impl AsceApo {
    pub fn new() -> Self {
        Self {
            engine: None,
            mmap: None,
            sample_rate: 48000,
            channels: 2,
            frame_size: 480, // 10ms at 48kHz
            locked: false,
            channel_buf: Vec::new(),
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
        self.engine = Some(
            CirrusEngineBuilder::new(self.sample_rate, fft_size)
                .with_config(config)
                .with_channels(self.channels)
                .build_default(),
        );

        // Pre-allocate channel scratch buffer for de-interleaving.
        // This avoids Vec allocation inside process() (RT safety).
        self.channel_buf = vec![0.0f32; frame_size];

        self.locked = true;
    }

    /// Release resources
    pub fn unlock_for_process(&mut self) {
        self.locked = false;
        self.engine = None;
    }

    /// Real-time audio processing. MUST be deterministic:
    /// - No allocation
    /// - No mutex/lock
    /// - No I/O
    /// - No panic (wrapped in catch_unwind by caller)
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        if !self.locked {
            // Not locked, pass through
            output[..input.len()].copy_from_slice(input);
            return;
        }

        // Check bypass via mmap
        if let Some(ref mmap) = self.mmap {
            if !mmap.config().is_enabled() {
                output[..input.len()].copy_from_slice(input);
                return;
            }

            // Hot-reload config changes (atomic read, no lock)
            self.maybe_update_config(mmap.config().version.load(
                std::sync::atomic::Ordering::Relaxed,
            ));
        }

        if let Some(ref mut engine) = self.engine {
            // Process each channel
            let frames = input.len() / self.channels as usize;
            let ch = self.channels as usize;

            if ch == 1 {
                // Mono: direct processing
                output[..frames].copy_from_slice(&input[..frames]);
                engine.process(&mut output[..frames]);
            } else {
                // Multi-channel: process each channel separately
                // De-interleave → process → re-interleave
                // Uses pre-allocated channel_buf (no allocation in hot path)
                for c in 0..ch {
                    // Extract channel into pre-allocated buffer
                    for f in 0..frames {
                        self.channel_buf[f] = input[f * ch + c];
                    }

                    engine.process(&mut self.channel_buf[..frames]);

                    // Write back
                    for f in 0..frames {
                        output[f * ch + c] = self.channel_buf[f];
                    }

                    // Reset between channels to prevent L→R state contamination.
                    // CirrusEngine is stateful (frame_index, damage, lattice, fields,
                    // validated all accumulate). Without reset, R channel would inherit
                    // L channel's processing history.
                    // TODO: Replace with proper stereo-native architecture
                    // (dual-engine + shared cross-channel, or interleaved processing).
                    if c == 0 {
                        engine.reset();
                    }
                }
            }

            // Update status via mmap
            if let Some(ref mmap) = self.mmap {
                let status = mmap.status();
                status.increment_frames();
                let damage = engine.damage_posterior();
                status.set_cutoff(Some(damage.cutoff.mean));
                status.set_clipping(damage.clipping.mean);
            }
        } else {
            output[..input.len()].copy_from_slice(input);
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
            }
        } else {
            EngineConfig::default()
        }
    }

    fn maybe_update_config(&mut self, _new_version: u32) {
        // TODO: track config version and only update on change
        // For now, update every frame (atomic reads are cheap)
        let config = self.load_config();
        if let Some(ref mut engine) = self.engine {
            engine.update_config(config);
        }
    }
}
