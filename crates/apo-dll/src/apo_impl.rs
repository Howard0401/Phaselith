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
    /// Previous frame's enabled state — used to detect transitions for crossfade.
    prev_enabled: bool,
    /// Frame counter for warmup phase. Engine output is blended from 0%→100%
    /// over the first WARMUP_FRAMES to let OLA/EMA/damage posteriors settle.
    process_frame_count: u64,
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
            prev_enabled: false,
            process_frame_count: 0,
        }
    }

    /// Number of frames to blend from dry→wet on engine startup.
    /// Lets OLA buffer fill, EMA smoothers converge, and damage posteriors
    /// accumulate before applying full enhancement.
    const WARMUP_FRAMES: u64 = 80;

    /// Check a buffer for NaN/Inf values. Returns true if the buffer is clean.
    #[inline]
    fn is_buffer_clean(buf: &[f32], len: usize) -> bool {
        for i in 0..len {
            if !buf[i].is_finite() {
                return false;
            }
        }
        true
    }

    /// Compute the blend factor for warmup phase.
    /// Returns 0.0 at frame 0, rises to 1.0 at WARMUP_FRAMES.
    #[inline]
    fn warmup_blend(&self) -> f32 {
        if self.process_frame_count >= Self::WARMUP_FRAMES {
            1.0
        } else {
            self.process_frame_count as f32 / Self::WARMUP_FRAMES as f32
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
        // Use frame_size (480) as max_frame_size — NOT fft_size (1024).
        // Using fft_size caused frame_params.host_block_size=1024 (wrong),
        // validated.data sized to 1024 (47% wasted/zeroed), and OLA drain
        // under-serving the actual 480-sample blocks.
        self.engine_l = Some(
            PhaselithEngineBuilder::new(self.sample_rate, self.frame_size)
                .with_config(config)
                .with_channels(1)
                .build_default(),
        );

        if self.channels >= 2 {
            self.engine_r = Some(
                PhaselithEngineBuilder::new(self.sample_rate, self.frame_size)
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
        self.process_frame_count = 0;

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
            let ver = mmap.config().version.load(std::sync::atomic::Ordering::Acquire);
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

        // Detect enabled state transition for crossfade
        let transitioning = enabled != self.prev_enabled;
        self.prev_enabled = enabled;
        self.process_frame_count += 1;

        // Warmup blend: gradually ramp from dry→wet over WARMUP_FRAMES.
        // During warmup the OLA buffer, EMA smoothers, and damage posteriors
        // are still settling — full-strength output can contain artifacts.
        let warmup = self.warmup_blend();

        if ch == 1 {
            // Mono: use L engine only — always process to keep state in sync
            if let Some(ref mut engine) = self.engine_l {
                self.channel_buf_l[..frames].copy_from_slice(&input[..frames]);
                engine.process(&mut self.channel_buf_l[..frames]);

                // Safety: NaN/Inf guard — if engine produced garbage, passthrough
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
                    // Apply warmup blend
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
        } else if ch >= 2 {
            // Stereo: dual-engine with symmetric cross-channel context.
            // ALWAYS process through engines to keep OLA/frame clock in sync.

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

            // Cross-channel context (one-frame delay)
            let cc = CrossChannelContext::from_lr(
                &self.dry_lr_saved[..frames],
                &self.dry_lr_saved[frames..frames * 2],
            );
            self.cross_channel_prev = Some(cc);

            // Safety: NaN/Inf guard — if either engine produced garbage, passthrough
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
                // Apply warmup blend
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
                // APO always uses FftOlaPilot: LegacyAdditive synthesis restarts
                // from n=0 every block, producing periodic discontinuities when
                // block_size (480) < fft_size (1024). FftOlaPilot uses proper
                // ISTFT + overlap-add for continuous, hop-aligned output.
                synthesis_mode: SynthesisMode::FftOlaPilot,
                ambience_preserve: 0.0,
            }
        } else {
            EngineConfig::default()
        }
    }
}
