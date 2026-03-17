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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

/// Global engine storage — persists engine state across APO instance recreations.
/// When Windows destroys an APO instance (unlock_for_process), engines are stored here.
/// When a new instance is created (lock_for_process), it takes the stored engines
/// instead of building new ones. This preserves OLA buffers, EMA smoothers, and
/// damage posteriors, eliminating the pop/quality-dip from engine restart.
struct StoredEngines {
    engine_l: PhaselithEngine,
    engine_r: Option<PhaselithEngine>,
    sample_rate: u32,
    frame_size: usize,
    cross_channel_prev: Option<CrossChannelContext>,
}

/// Recent input audio for priming fresh engines with real audio instead of silence.
/// Updated on every process() call. When a fresh engine is created (reused=false
/// because old instance hasn't unlocked yet), these blocks are fed to the engine
/// so its OLA state matches what a settled engine would have.
struct StoredPrimeAudio {
    /// Ring buffer of recent L channel blocks (PRIME_FRAMES entries)
    blocks_l: Vec<Vec<f32>>,
    /// Ring buffer of recent R channel blocks (PRIME_FRAMES entries)
    blocks_r: Vec<Vec<f32>>,
    /// Write cursor into the ring buffer
    cursor: usize,
    /// Number of blocks stored (up to PRIME_FRAMES)
    count: usize,
    sample_rate: u32,
    frame_size: usize,
}

static STORED_ENGINES: Mutex<Option<StoredEngines>> = Mutex::new(None);
static STORED_PRIME: Mutex<Option<StoredPrimeAudio>> = Mutex::new(None);

/// Lock-free last output storage — updated by every process() call (RT-safe).
/// New instances read these to seed their click gate, enabling immediate
/// boundary discontinuity detection from frame 1 instead of waiting CLICK_GATE_DELAY.
static LAST_OUTPUT_L_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_OUTPUT_R_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_OUTPUT_VALID: AtomicBool = AtomicBool::new(false);

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
    /// Frame counter since lock_for_process. Used for click gate delay.
    process_frame_count: u64,
    /// Shutdown fadeout: when true, process() applies a linear fadeout over
    /// one block and then enters bypass. Set by requesting shutdown.
    shutting_down: bool,
    /// When true, warmup blend fades from silence→wet instead of dry→wet.
    /// Set by gap detection to mute the output during OLA settling, avoiding
    /// the brief pop that would occur from stale OLA buffer data.
    gap_mute_active: bool,
    /// Last output sample per channel — used by click gate to detect
    /// boundary discontinuities between consecutive blocks.
    last_output_l: f32,
    last_output_r: f32,
    /// When true, click gate is armed from frame 1 (skip CLICK_GATE_DELAY).
    /// Set when last_output is seeded from global atomic storage, meaning
    /// we have valid reference values for boundary discontinuity detection.
    click_gate_armed: bool,
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
            shutting_down: false,
            gap_mute_active: false,
            last_output_l: 0.0,
            last_output_r: 0.0,
            click_gate_armed: false,
        }
    }

    /// Number of frames to delay click gate activation after instance start
    /// or click gate recovery. During this window, output is full wet with no
    /// blending — PRIME_FRAMES already settle the OLA buffer. The delay prevents
    /// false click-gate triggers because last_output_l/r starts at 0.0.
    const CLICK_GATE_DELAY: u64 = 6;

    /// Number of silent frames to prime engines during lock_for_process().
    /// This fills the OLA buffer so the first real audio block doesn't start
    /// from an empty accumulator (which causes a startup pop/click).
    /// Need at least ceil(fft_size / hop_size) = ceil(1024/256) = 4 frames
    /// to fully populate the OLA overlap. Use 6 for margin.
    const PRIME_FRAMES: usize = 6;

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

    /// Compute the blend factor for click gate recovery.
    /// Returns 1.0 normally (full wet output, no warmup blend).
    /// Only returns < 1.0 when gap_mute_active is true (click gate recovery):
    /// smoothstep from 0→1 over CLICK_GATE_DELAY frames.
    #[inline]
    fn recovery_blend(&self) -> f32 {
        if !self.gap_mute_active || self.process_frame_count >= Self::CLICK_GATE_DELAY {
            1.0
        } else {
            let t = self.process_frame_count as f32 / Self::CLICK_GATE_DELAY as f32;
            t * t * (3.0 - 2.0 * t)
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

        // Pre-allocate scratch buffers (no allocation in hot path)
        self.channel_buf_l = vec![0.0f32; frame_size];
        self.channel_buf_r = vec![0.0f32; frame_size];
        self.dry_lr_saved = vec![0.0f32; frame_size * 2]; // L dry + R dry
        self.last_config_version = 0;
        self.process_frame_count = 0;
        self.gap_mute_active = false;
        self.click_gate_armed = false;

        // Try to reuse stored engines from a previous instance.
        // This preserves OLA buffers, EMA smoothers, and damage posteriors
        // across APO instance recreations (YY Voice join/leave, etc.),
        // eliminating the pop/quality-dip from engine restart.
        let mut reused = false;
        if let Ok(mut guard) = STORED_ENGINES.lock() {
            if let Some(stored) = guard.as_ref() {
                if stored.sample_rate == self.sample_rate && stored.frame_size == frame_size {
                    let stored = guard.take().unwrap();
                    self.engine_l = Some(stored.engine_l);
                    self.engine_r = stored.engine_r;
                    self.cross_channel_prev = stored.cross_channel_prev;
                    reused = true;
                }
            }
        }

        if !reused {
            // Load config from mmap, or use defaults
            let config = self.load_config();

            // Dual-engine: each engine processes one mono channel independently.
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

            self.cross_channel_prev = None;

            // Prime engines to fill OLA buffer.
            // Try to use stored real audio instead of silence — this makes the
            // fresh engine's OLA state match what a settled engine would have,
            // eliminating the pop from engine state discontinuity.
            {
                let mut used_real_audio = false;
                if let Ok(mut guard) = STORED_PRIME.lock() {
                    if let Some(ref prime) = *guard {
                        if prime.sample_rate == self.sample_rate
                            && prime.frame_size == frame_size
                            && prime.count >= Self::PRIME_FRAMES
                        {
                            // Feed stored blocks in order (oldest first)
                            let start = prime.cursor.wrapping_sub(prime.count) % Self::PRIME_FRAMES;
                            for i in 0..Self::PRIME_FRAMES {
                                let idx = (start + i) % Self::PRIME_FRAMES;
                                let mut buf = prime.blocks_l[idx].clone();
                                if let Some(ref mut engine) = self.engine_l {
                                    engine.process(&mut buf);
                                }
                                if self.channels >= 2 {
                                    let mut buf_r = prime.blocks_r[idx].clone();
                                    if let Some(ref mut engine) = self.engine_r {
                                        engine.process(&mut buf_r);
                                    }
                                }
                            }
                            used_real_audio = true;
                            apo_log!("PRIME: used {} blocks of real audio", Self::PRIME_FRAMES);
                        }
                    }
                }

                if !used_real_audio {
                    // Fallback: prime with silence (first-ever startup)
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
                    apo_log!("PRIME: used silence (no stored audio available)");
                }
            }
        }

        // Seed last_output from global atomic storage so click gate can detect
        // boundary discontinuities from frame 1 (instead of waiting CLICK_GATE_DELAY).
        // Critical for concurrent instances: when Windows switches from instance A
        // to instance B, B's first output may differ from A's last output → pop.
        // With seeded last_output, click gate catches this immediately.
        if LAST_OUTPUT_VALID.load(Ordering::Acquire) {
            self.last_output_l = f32::from_bits(LAST_OUTPUT_L_BITS.load(Ordering::Relaxed));
            self.last_output_r = f32::from_bits(LAST_OUTPUT_R_BITS.load(Ordering::Relaxed));
            self.click_gate_armed = true;
            apo_log!("SEEDED last_output=({:.6},{:.6})", self.last_output_l, self.last_output_r);
        } else {
            self.last_output_l = 0.0;
            self.last_output_r = 0.0;
        }

        self.locked = true;
        apo_log!("lock_for_process done, frame_size={}, reused={}", frame_size, reused);
    }

    /// Release resources.
    /// Stores engines in global static for reuse by next instance.
    /// Sets shutting_down flag so the next process() call does a clean passthrough
    /// instead of abruptly cutting from wet to silence.
    pub fn unlock_for_process(&mut self) {
        apo_log!("unlock_for_process, frame_count={}", self.process_frame_count);
        self.shutting_down = true;
        self.locked = false;

        // Store engines for reuse by the next APO instance.
        // This preserves OLA/EMA/posterior state across instance recreations.
        if let Some(el) = self.engine_l.take() {
            if let Ok(mut guard) = STORED_ENGINES.lock() {
                *guard = Some(StoredEngines {
                    engine_l: el,
                    engine_r: self.engine_r.take(),
                    sample_rate: self.sample_rate,
                    frame_size: self.frame_size,
                    cross_channel_prev: self.cross_channel_prev,
                });
            }
        }
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

        // Shutdown fadeout: apply a linear fade from current level to dry passthrough
        // over one block, then switch to full bypass. This prevents the abrupt
        // discontinuity when APO is being removed/uninstalled.
        if self.shutting_down {
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

        // Recovery blend: normally 1.0 (full wet, no warmup needed — PRIME_FRAMES
        // already settled the OLA buffer). Only < 1.0 during click gate recovery
        // (silence→wet fade-in after a detected artifact).
        let recovery = self.recovery_blend();
        if recovery >= 1.0 && self.gap_mute_active {
            self.gap_mute_active = false;
            apo_log!("CLICK_RECOVERY_DONE: frame={}, last_out=({:.6},{:.6})",
                self.process_frame_count, self.last_output_l, self.last_output_r);
        }

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
                    // Full wet output — no warmup blend needed.
                    // PRIME_FRAMES already settled the OLA buffer during lock_for_process.
                    // Only scale down during click gate recovery (gap_mute_active).
                    if recovery >= 1.0 {
                        output[..frames].copy_from_slice(&self.channel_buf_l[..frames]);
                    } else {
                        for f in 0..frames {
                            output[f] = self.channel_buf_l[f] * recovery;
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
                // Full wet output — no warmup blend needed.
                // PRIME_FRAMES already settled the OLA buffer during lock_for_process.
                // Only scale down during click gate recovery (gap_mute_active).
                if recovery >= 1.0 {
                    for f in 0..frames {
                        output[f * 2] = self.channel_buf_l[f];
                        output[f * 2 + 1] = self.channel_buf_r[f];
                    }
                } else {
                    for f in 0..frames {
                        output[f * 2] = self.channel_buf_l[f] * recovery;
                        output[f * 2 + 1] = self.channel_buf_r[f] * recovery;
                    }
                }
            } else {
                output[..copy_len].copy_from_slice(&input[..copy_len]);
            }
        }

        // ═══ Click Gate (universal output safety) ═══
        // After all output is written, check for boundary discontinuities.
        // If detected, zero the block and activate gap_mute for smooth recovery.
        // This catches ALL pop/click sources: engine restart, audio graph changes,
        // stale OLA state, etc. — same approach as Apogee's output gate.
        // Click gate armed from frame 1 when last_output was seeded from global,
        // otherwise wait CLICK_GATE_DELAY frames (last_output starts at 0.0, which
        // would cause false positives against non-zero first output).
        let click_gate_ready = if self.click_gate_armed {
            self.process_frame_count >= 1
        } else {
            self.process_frame_count >= Self::CLICK_GATE_DELAY
        };
        if enabled && !transitioning && click_gate_ready {
            let (first_l, first_r, last_l, last_r) = if ch >= 2 && frames > 0 {
                (output[0], output[1], output[(frames - 1) * 2], output[(frames - 1) * 2 + 1])
            } else if frames > 0 {
                (output[0], 0.0, output[frames - 1], 0.0)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };

            let boundary_disc = (first_l - self.last_output_l).abs()
                .max((first_r - self.last_output_r).abs());

            // Adaptive threshold: compare boundary jump to max within-block jump.
            // A click has a boundary disc much larger than any within-block diff.
            // This avoids false positives from legitimate musical transients.
            let mut max_within = 0.0f32;
            if ch >= 2 {
                for f in 1..frames {
                    let dl = (output[f * 2] - output[(f - 1) * 2]).abs();
                    let dr = (output[f * 2 + 1] - output[(f - 1) * 2 + 1]).abs();
                    let d = dl.max(dr);
                    if d > max_within { max_within = d; }
                }
            } else {
                for f in 1..frames {
                    let d = (output[f] - output[f - 1]).abs();
                    if d > max_within { max_within = d; }
                }
            }

            // Click if boundary exceeds adaptive threshold:
            // - Must be > 1.5× the largest within-block jump + absolute floor
            // - Absolute floor (0.05) prevents triggering on near-silence
            const CLICK_RATIO: f32 = 1.5;
            const CLICK_FLOOR: f32 = 0.01;
            if boundary_disc > max_within * CLICK_RATIO + CLICK_FLOOR {
                // Mute this block — zero output for clean gate
                apo_log!("CLICK_GATE: disc={:.6}, max_within={:.6}, threshold={:.6}, frame={}",
                    boundary_disc, max_within, max_within * CLICK_RATIO + CLICK_FLOOR,
                    self.process_frame_count);
                output[..copy_len].fill(0.0);
                self.gap_mute_active = true;
                self.process_frame_count = 0;
                // Signal UI via mmap
                if let Some(ref mmap) = self.mmap {
                    mmap.status().increment_pop_muted();
                }
            }

            // Track last output samples for next block's boundary check
            if ch >= 2 && frames > 0 {
                self.last_output_l = output[(frames - 1) * 2];
                self.last_output_r = output[(frames - 1) * 2 + 1];
            } else if frames > 0 {
                self.last_output_l = output[frames - 1];
                self.last_output_r = 0.0;
            }
        } else if frames > 0 {
            // Track last output even during warmup/transition/disabled
            if ch >= 2 {
                self.last_output_l = output[(frames - 1) * 2];
                self.last_output_r = output[(frames - 1) * 2 + 1];
            } else {
                self.last_output_l = output[frames - 1];
                self.last_output_r = 0.0;
            }
        }

        // Update global last_output atomics (lock-free, RT-safe).
        // Other instances read these during lock_for_process to seed their click gate.
        if frames > 0 {
            LAST_OUTPUT_L_BITS.store(self.last_output_l.to_bits(), Ordering::Relaxed);
            LAST_OUTPUT_R_BITS.store(self.last_output_r.to_bits(), Ordering::Relaxed);
            LAST_OUTPUT_VALID.store(true, Ordering::Release);
        }

        // Store recent input audio for priming future fresh engines.
        // Uses try_lock (non-blocking) — if lock is held, skip this frame.
        if self.process_frame_count > 0 {
            if let Ok(mut guard) = STORED_PRIME.try_lock() {
                let prime = guard.get_or_insert_with(|| StoredPrimeAudio {
                    blocks_l: (0..Self::PRIME_FRAMES).map(|_| vec![0.0f32; self.frame_size]).collect(),
                    blocks_r: (0..Self::PRIME_FRAMES).map(|_| vec![0.0f32; self.frame_size]).collect(),
                    cursor: 0,
                    count: 0,
                    sample_rate: self.sample_rate,
                    frame_size: self.frame_size,
                });
                if prime.sample_rate == self.sample_rate && prime.frame_size == self.frame_size {
                    let idx = prime.cursor % Self::PRIME_FRAMES;
                    // Store de-interleaved input
                    if ch >= 2 {
                        for f in 0..frames {
                            prime.blocks_l[idx][f] = input[f * 2];
                            prime.blocks_r[idx][f] = input[f * 2 + 1];
                        }
                    } else {
                        prime.blocks_l[idx][..frames].copy_from_slice(&input[..frames]);
                        prime.blocks_r[idx].iter_mut().for_each(|v| *v = 0.0);
                    }
                    prime.cursor = prime.cursor.wrapping_add(1);
                    if prime.count < Self::PRIME_FRAMES {
                        prime.count += 1;
                    }
                }
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

            // Measure wet/dry difference: RMS of (output - input) in dB.
            // This tells the user whether the algorithm is actually modifying audio.
            if enabled && !self.shutting_down {
                let mut diff_sum = 0.0f32;
                let n = copy_len.min(input.len()).min(output.len());
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
                // Reduced from 1.0: APO receives clean post-mixer signal,
                // aggressive pre-echo suppression creates spectral splatter
                // (tanh time-domain gain window → broadband HF artifacts).
                // 0.4 preserves audible pre-echo reduction while avoiding
                // sibilance at high Transient Repair settings.
                pre_echo_transient_scaling: 0.4,
                declip_transient_scaling: 1.0,
                delayed_transient_repair: false,
                // Standard mode (FFT 1024, hop 256) matches Chrome extension.
                // Engine sub-block processing splits APO blocks (528) into
                // ≤ hop_size chunks, guaranteeing hops_this_block ≤ 1 and
                // preventing M5 OLA multi-hop artifacts.
                phase_mode: PhaseMode::Linear,
                quality_mode: QualityMode::Standard,
                // Read filter style from mmap and apply corresponding StyleConfig.
                // When Custom (3), read individual 6-axis values from mmap.
                // When preset (0/1/2), derive from preset (ignore mmap axis values).
                style: {
                    let fs_val = sc.filter_style.load(std::sync::atomic::Ordering::Relaxed) as u32;
                    let fs = phaselith_dsp_core::config::FilterStyle::from_u32(fs_val);
                    if fs.is_preset() {
                        fs.to_style_config()
                    } else {
                        // Custom: read 6 axes directly from mmap
                        StyleConfig::new(
                            sc.warmth(),
                            sc.air_brightness(),
                            sc.smoothness(),
                            sc.spatial_spread(),
                            sc.impact_gain(),
                            sc.body(),
                        )
                    }
                },
                // LegacyAdditive (sum-of-cosines) matches Chrome extension default.
                // FftOlaPilot (ISTFT+OLA) produces audible HF sizzling artifacts
                // with Standard-mode's coarser 1024-point FFT resolution.
                synthesis_mode: SynthesisMode::LegacyAdditive,
                ambience_preserve: 0.0,
                filter_style: {
                    let fs_val = sc.filter_style.load(std::sync::atomic::Ordering::Relaxed) as u32;
                    phaselith_dsp_core::config::FilterStyle::from_u32(fs_val)
                },
            }
        } else {
            EngineConfig::default()
        }
    }
}
