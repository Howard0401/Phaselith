// Phaselith Audio Processing Object implementation.
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
use phaselith_dsp_core::config::{EngineConfig, PhaseMode, QualityMode, StyleConfig, SynthesisMode};
use phaselith_dsp_core::engine::{PhaselithEngine, PhaselithEngineBuilder};
use phaselith_dsp_core::types::CrossChannelContext;

/// The Phaselith APO instance.
/// Created by ClassFactory, one per audio stream.
///
/// Dual-engine architecture (aligned with browser WASM bridge):
/// - Two independent mono engines (L/R) with no state contamination
/// - Symmetric one-frame-delayed cross-channel context
/// - Each engine sees the same CrossChannelContext from the previous frame
pub struct PhaselithApo {
    engine_l: Option<PhaselithEngine>,
    engine_r: Option<PhaselithEngine>,
    mmap: Option<MmapIpc>,
    sample_rate: u32,
    channels: u16,
    frame_size: usize,
    locked: bool,
    /// If true, engine init failed — pure passthrough, never attempt DSP.
    bypass_mode: bool,
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

impl PhaselithApo {
    pub fn new() -> Self {
        Self {
            engine_l: None,
            engine_r: None,
            mmap: None,
            sample_rate: 48000,
            channels: 2,
            frame_size: 480, // 10ms at 48kHz
            locked: false,
            bypass_mode: false,
            channel_buf_l: Vec::new(),
            channel_buf_r: Vec::new(),
            dry_lr_saved: Vec::new(),
            cross_channel_prev: None,
            last_config_version: 0,
        }
    }

    /// Enter permanent bypass mode (engine init failed).
    /// Audio passes through unmodified — never crashes.
    pub fn set_bypass_mode(&mut self) {
        self.bypass_mode = true;
        self.engine_l = None;
        self.engine_r = None;
        self.locked = true;
    }

    /// Called during APO initialization (non-real-time)
    pub fn initialize(&mut self, sample_rate: u32, channels: u16) {
        self.sample_rate = sample_rate;
        self.channels = channels;

        // Try to open mmap IPC (non-fatal — logs error but continues in bypass)
        match MmapIpc::open_or_create() {
            Ok(ipc) => self.mmap = Some(ipc),
            Err(e) => {
                // Log structured error but don't crash — APO continues without IPC
                eprintln!("Phaselith APO: mmap IPC init failed: {e}");
                self.mmap = None;
            }
        }
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
        // Use fft_size (1024) as max_frame_size — same as Chrome WASM bridge.
        self.engine_l = Some(
            PhaselithEngineBuilder::new(self.sample_rate, fft_size)
                .with_config(config)
                .with_channels(1)
                .build_default(),
        );

        if self.channels >= 2 {
            self.engine_r = Some(
                PhaselithEngineBuilder::new(self.sample_rate, fft_size)
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
        // Safe copy helper — always works even if sizes mismatch
        let copy_len = input.len().min(output.len());
        if copy_len == 0 { return; }

        if !self.locked || self.bypass_mode {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }

        // Read enabled state and version from mmap first (immutable borrow),
        // then update config (mutable borrow). ALWAYS process through engines
        // to keep internal state — OLA buffer, frame clock — in sync.
        let (enabled, mmap_version) = if let Some(ref mmap) = self.mmap {
            let ver = mmap.config().version.load(std::sync::atomic::Ordering::Relaxed);
            let en = mmap.config().is_enabled();
            (en, ver)
        } else {
            (false, 0)
        };
        self.maybe_update_config(mmap_version);

        let ch = self.channels as usize;
        if ch == 0 {
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }
        let frames = input.len() / ch;

        // Validate scratch buffers are large enough
        if frames > self.channel_buf_l.len() || frames > self.channel_buf_r.len()
            || frames * 2 > self.dry_lr_saved.len()
        {
            // Buffer too small — passthrough rather than crash
            output[..copy_len].copy_from_slice(&input[..copy_len]);
            return;
        }

        if ch == 1 {
            // Mono: use L engine only — always process to keep state in sync
            if let Some(ref mut engine) = self.engine_l {
                self.channel_buf_l[..frames].copy_from_slice(&input[..frames]);
                engine.process(&mut self.channel_buf_l[..frames]);
                if enabled {
                    output[..frames].copy_from_slice(&self.channel_buf_l[..frames]);
                } else {
                    output[..frames].copy_from_slice(&input[..frames]);
                }
            } else {
                output[..frames].copy_from_slice(&input[..frames]);
            }
        } else if ch >= 2 {
            // Stereo: dual-engine with symmetric cross-channel context.
            // ALWAYS process through engines to keep OLA/frame clock in sync.
            // Output dry signal when disabled.

            // Validate we have enough samples for stereo de-interleave
            if input.len() < frames * 2 || output.len() < frames * 2 {
                output[..copy_len].copy_from_slice(&input[..copy_len]);
                return;
            }

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

            // Re-interleave output: use wet (processed) when enabled, dry when disabled
            if enabled {
                for f in 0..frames {
                    output[f * 2] = self.channel_buf_l[f];
                    output[f * 2 + 1] = self.channel_buf_r[f];
                }
            } else {
                output[..copy_len].copy_from_slice(&input[..copy_len]);
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

    /// Hot-reload config when mmap version changes (called from RT thread).
    /// Only reads atomics — no alloc, no lock, no I/O.
    fn maybe_update_config(&mut self, current_version: u32) {
        if current_version == self.last_config_version {
            return;
        }
        self.last_config_version = current_version;

        let config = self.load_config();
        if let Some(ref mut engine) = self.engine_l {
            engine.update_config(config);
        }
        if let Some(ref mut engine) = self.engine_r {
            engine.update_config(config);
        }
    }

    fn load_config(&self) -> EngineConfig {
        if let Some(ref mmap) = self.mmap {
            let sc = mmap.config();
            EngineConfig {
                // Always tell engine it's enabled — APO handles wet/dry switching
                // externally. Engine must always process to keep OLA/frame clock in sync.
                enabled: true,
                strength: sc.compensation_strength(),
                hf_reconstruction: sc.hf_reconstruction(),
                dynamics: sc.dynamics_restoration(),
                transient: sc.transient_repair(),
                pre_echo_transient_scaling: 1.0,
                declip_transient_scaling: 1.0,
                delayed_transient_repair: false,
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
