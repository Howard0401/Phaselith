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
use asce_dsp_core::config::{EngineConfig, PhaseMode, QualityMode};
use asce_dsp_core::engine::{CirrusEngine, CirrusEngineBuilder};

/// The ASCE APO instance.
/// Created by ClassFactory, one per audio stream.
pub struct AsceApo {
    engine: Option<CirrusEngine>,
    mmap: Option<MmapIpc>,
    sample_rate: u32,
    channels: u16,
    frame_size: usize,
    locked: bool,
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
                for c in 0..ch {
                    // Extract channel
                    let mut channel_buf: Vec<f32> = (0..frames)
                        .map(|f| input[f * ch + c])
                        .collect();

                    engine.process(&mut channel_buf);

                    // Write back
                    for f in 0..frames {
                        output[f * ch + c] = channel_buf[f];
                    }

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
