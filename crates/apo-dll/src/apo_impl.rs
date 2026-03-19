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
use std::time::Instant;

/// Global engine storage — persists engine state across APO instance recreations.
/// When Windows destroys the APO instance (unlock_for_process), engines are stored here.
/// When a new instance is created (lock_for_process), it takes the stored engines
/// instead of building new ones. This preserves OLA buffers, EMA smoothers, and
/// damage posteriors, eliminating the pop/quality-dip from engine restart.
///
/// SAFETY: As an EFX (endpoint effect), only one instance exists per endpoint,
/// so these globals are never accessed concurrently by multiple instances.
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

/// Stored last processed output block — used for crossfade when fresh engines
/// are created. Updated every process() call via try_lock (non-blocking, RT-safe).
struct StoredOutputBlock {
    data: Vec<f32>,   // interleaved output, one block
    channels: u16,
    frame_size: usize,
}

static STORED_ENGINES: Mutex<Option<StoredEngines>> = Mutex::new(None);
static STORED_PRIME: Mutex<Option<StoredPrimeAudio>> = Mutex::new(None);
static STORED_OUTPUT: Mutex<Option<StoredOutputBlock>> = Mutex::new(None);

/// Timestamp of last unlock_for_process — used for rapid rebuild detection.
/// If a new lock_for_process happens within RAPID_REBUILD_WINDOW of the last unlock,
/// we skip click gate and reduce prime frames to minimize audible interruption.
static LAST_UNLOCK_TIME: Mutex<Option<Instant>> = Mutex::new(None);
/// Format of last unlocked instance — only fast-path if format matches.
static LAST_UNLOCK_FORMAT: Mutex<Option<(u32, usize)>> = Mutex::new(None); // (sample_rate, frame_size)

/// Lock-free last output storage — updated by every process() call (RT-safe).
/// New instances read these to seed their click gate, enabling immediate
/// boundary discontinuity detection from frame 1 instead of waiting CLICK_GATE_DELAY.
static LAST_OUTPUT_L_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_OUTPUT_R_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_OUTPUT_VALID: AtomicBool = AtomicBool::new(false);

/// The Phaselith APO instance.
/// Registered as EFX (endpoint effect): single instance per audio endpoint,
/// processes the final mixed signal from all streams before DAC output.
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
    /// Stored output block from previous instance for crossfade (fresh engine only).
    /// When set, first few frames blend from this old output to new engine output.
    crossfade_from: Option<Vec<f32>>,
    /// Remaining frames of crossfade (counts down to 0).
    crossfade_frames_left: u32,
    /// Number of consecutive zero-input blocks that were faded out.
    /// When > 0 and real audio resumes, crossfade from silence into engine output
    /// to prevent boundary discontinuity at the zero→audio transition.
    zero_blocks_count: u32,
    /// Timestamp of last process() call — used to detect DPC latency spikes.
    /// If the gap between calls exceeds expected_period × DPC_SPIKE_RATIO,
    /// the audio thread was starved (buffer underrun).
    last_process_time: Option<Instant>,
    /// When true, the previous frame had a DPC spike — apply crossfade on this frame.
    dpc_spike_recovery: bool,
    /// Saved last output before DPC spike for crossfade source.
    dpc_saved_l: f32,
    dpc_saved_r: f32,
    /// Adaptive DPC threshold: tracks recent spike count to auto-switch
    /// between normal mode (2.5) and high-latency mode (15.0).
    dpc_spike_recent_count: u32,
    /// Frame counter for DPC spike window — resets every DPC_ADAPTIVE_WINDOW frames.
    dpc_window_frame_count: u64,
    /// Whether we're currently in high-latency DPC mode.
    dpc_high_latency_mode: bool,
    /// Frames since last spike in high-latency mode — used to auto-recover.
    dpc_calm_frame_count: u64,
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
            crossfade_from: None,
            crossfade_frames_left: 0,
            zero_blocks_count: 0,
            last_process_time: None,
            dpc_spike_recovery: false,
            dpc_saved_l: 0.0,
            dpc_saved_r: 0.0,
            dpc_spike_recent_count: 0,
            dpc_window_frame_count: 0,
            dpc_high_latency_mode: false,
            dpc_calm_frame_count: 0,
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
    const PRIME_FRAMES: usize = 32;

    /// Number of frames to crossfade from old instance output to new engine output.
    /// Only used for fresh engines (reused=false). 32 frames × 11ms = 352ms.
    /// During crossfade the engine processes real audio → EMA/posteriors converge.
    const CROSSFADE_FRAMES: u32 = 32;

    /// Number of startup frames where zero-input protection is active.
    /// Windows sends 1-3 zero blocks during audio graph reconfiguration.
    /// Protection covers these plus the first real audio block after zeros.
    const STARTUP_GUARD_FRAMES: u64 = 8;

    /// DPC latency spike detection thresholds.
    /// Normal mode: sensitive detection, mute on small spikes for best quality.
    /// High-latency mode: only mute on severe spikes to avoid frequent interruptions.
    const DPC_SPIKE_RATIO_NORMAL: f32 = 2.5;
    const DPC_SPIKE_RATIO_HIGH_LATENCY: f32 = 15.0;
    /// Number of spikes within the adaptive window to trigger high-latency mode.
    const DPC_ADAPTIVE_SPIKE_THRESHOLD: u32 = 3;
    /// Window size in frames for counting spikes (~30 seconds at 480 samples/10ms).
    const DPC_ADAPTIVE_WINDOW: u64 = 3000;
    /// Frames without spike to recover from high-latency mode (~60 seconds).
    const DPC_CALM_RECOVERY: u64 = 6000;

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
        // Initialize prev_enabled = true so the first frame doesn't trigger
        // a false dry→wet transition. Without this, click gate is skipped on
        // frame 1 (transitioning=true), allowing boundary discontinuities through.
        self.prev_enabled = true;

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
            // hop_size=120 (4x STFT analysis per 480-sample frame)
            // sub-block=1 (per-sample OLA readout — maximum analog smoothness)
            const APO_SUB_BLOCK: usize = 1;
            self.engine_l = Some(
                PhaselithEngineBuilder::new(self.sample_rate, self.frame_size)
                    .with_config(config)
                    .with_channels(1)
                    .with_max_sub_block(APO_SUB_BLOCK)
                    .build_default(),
            );

            if self.channels >= 2 {
                self.engine_r = Some(
                    PhaselithEngineBuilder::new(self.sample_rate, self.frame_size)
                        .with_config(config)
                        .with_channels(1)
                        .with_max_sub_block(APO_SUB_BLOCK)
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

        // Pre-allocate STORED_PRIME if not yet initialized, so the RT path
        // never needs to allocate. This is non-RT (lock_for_process).
        if let Ok(mut guard) = STORED_PRIME.lock() {
            if guard.is_none() {
                *guard = Some(StoredPrimeAudio {
                    blocks_l: (0..Self::PRIME_FRAMES).map(|_| vec![0.0f32; frame_size]).collect(),
                    blocks_r: (0..Self::PRIME_FRAMES).map(|_| vec![0.0f32; frame_size]).collect(),
                    cursor: 0,
                    count: 0,
                    sample_rate: self.sample_rate,
                    frame_size,
                });
            }
        }

        // Detect rapid rebuild: if last unlock was within 500ms with same format,
        // this is a voice-chat-induced rebuild (YY, Discord, Teams). Skip click gate
        // and start at full volume to avoid audible interruption.
        let rapid_rebuild = {
            let mut is_rapid = false;
            if let Ok(guard) = LAST_UNLOCK_TIME.lock() {
                if let Some(last_time) = *guard {
                    let elapsed = last_time.elapsed();
                    if elapsed.as_millis() < 500 {
                        // Check format matches
                        if let Ok(fmt_guard) = LAST_UNLOCK_FORMAT.lock() {
                            if let Some((sr, fs)) = *fmt_guard {
                                if sr == self.sample_rate && fs == frame_size {
                                    is_rapid = true;
                                }
                            }
                        }
                    }
                }
            }
            is_rapid
        };

        // Seed last_output from global atomic storage so click gate can detect
        // boundary discontinuities from frame 1 (instead of waiting CLICK_GATE_DELAY).
        if LAST_OUTPUT_VALID.load(Ordering::Acquire) {
            self.last_output_l = f32::from_bits(LAST_OUTPUT_L_BITS.load(Ordering::Relaxed));
            self.last_output_r = f32::from_bits(LAST_OUTPUT_R_BITS.load(Ordering::Relaxed));
            if rapid_rebuild {
                // Rapid rebuild: skip click gate entirely — we have valid last_output
                // and the audio graph is just being restructured, not a real start.
                self.click_gate_armed = false;
                self.gap_mute_active = false;
                self.process_frame_count = Self::CLICK_GATE_DELAY + 1; // skip delay
                apo_log!("RAPID_REBUILD: skipping click gate, last_out=({:.6},{:.6})",
                    self.last_output_l, self.last_output_r);
            } else {
                self.click_gate_armed = true;
                apo_log!("SEEDED last_output=({:.6},{:.6})", self.last_output_l, self.last_output_r);
            }
        } else {
            self.last_output_l = 0.0;
            self.last_output_r = 0.0;
        }

        // Log diagnostics for fresh engine — help determine if pops are from
        // our DSP or from the system-level audio graph switch.
        if !reused {
            apo_log!("FRESH_ENGINE: primed with {} frames, click_gate_armed={}, rapid_rebuild={}",
                Self::PRIME_FRAMES, self.click_gate_armed, rapid_rebuild);
        }

        self.locked = true;
        apo_log!("lock_for_process done, frame_size={}, reused={}, rapid_rebuild={}",
            frame_size, reused, rapid_rebuild);
    }

    /// Release resources.
    /// Stores engines in global static for reuse by next instance.
    /// Sets shutting_down flag so the next process() call does a clean passthrough
    /// instead of abruptly cutting from wet to silence.
    pub fn unlock_for_process(&mut self) {
        apo_log!("unlock_for_process, frame_count={}", self.process_frame_count);
        self.shutting_down = true;
        self.locked = false;

        // Record unlock timestamp + format for rapid rebuild detection.
        if let Ok(mut guard) = LAST_UNLOCK_TIME.lock() {
            *guard = Some(Instant::now());
        }
        if let Ok(mut guard) = LAST_UNLOCK_FORMAT.lock() {
            *guard = Some((self.sample_rate, self.frame_size));
        }

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

        // ═══ DPC latency spike detection ═══
        // Measure time between consecutive process() calls. Normal interval is
        // frame_size / sample_rate (e.g., 528/48000 = 11ms). If the gap is much
        // larger, the audio thread was starved by a DPC spike (network, GPU, USB
        // driver hogging CPU). The DAC ran out of samples → buffer underrun →
        // audible pop. We detect this and crossfade the next block to smooth it.
        {
            let now = Instant::now();

            // Adaptive DPC window: reset spike counter every DPC_ADAPTIVE_WINDOW frames
            self.dpc_window_frame_count += 1;
            if self.dpc_window_frame_count >= Self::DPC_ADAPTIVE_WINDOW {
                self.dpc_window_frame_count = 0;
                self.dpc_spike_recent_count = 0;
            }

            // Select threshold based on current mode
            let threshold = if self.dpc_high_latency_mode {
                Self::DPC_SPIKE_RATIO_HIGH_LATENCY
            } else {
                Self::DPC_SPIKE_RATIO_NORMAL
            };

            if let Some(last_time) = self.last_process_time {
                let elapsed_us = now.duration_since(last_time).as_micros() as f32;
                let expected_us = (frames as f32 / self.sample_rate as f32) * 1_000_000.0;
                let ratio = elapsed_us / expected_us;

                // Count spikes at normal threshold regardless of current mode
                if ratio > Self::DPC_SPIKE_RATIO_NORMAL && self.process_frame_count > 3 {
                    self.dpc_spike_recent_count += 1;
                    self.dpc_calm_frame_count = 0;
                } else {
                    self.dpc_calm_frame_count += 1;
                }

                // Auto-switch to high-latency mode
                if !self.dpc_high_latency_mode
                    && self.dpc_spike_recent_count >= Self::DPC_ADAPTIVE_SPIKE_THRESHOLD
                {
                    self.dpc_high_latency_mode = true;
                    apo_log!("DPC_ADAPTIVE: switched to HIGH LATENCY mode ({} spikes in window)",
                        self.dpc_spike_recent_count);
                }

                // Auto-recover to normal mode
                if self.dpc_high_latency_mode
                    && self.dpc_calm_frame_count >= Self::DPC_CALM_RECOVERY
                {
                    self.dpc_high_latency_mode = false;
                    self.dpc_spike_recent_count = 0;
                    apo_log!("DPC_ADAPTIVE: recovered to NORMAL mode (calm for {} frames)",
                        self.dpc_calm_frame_count);
                }

                // Mute only if ratio exceeds current threshold
                if ratio > threshold && self.process_frame_count > 3 {
                    apo_log!("DPC_SPIKE: elapsed={:.0}us, expected={:.0}us, ratio={:.2}, frame={}, mode={}",
                        elapsed_us, expected_us, ratio, self.process_frame_count,
                        if self.dpc_high_latency_mode { "HIGH_LATENCY" } else { "NORMAL" });
                    self.dpc_spike_recovery = true;
                    self.dpc_saved_l = self.last_output_l;
                    self.dpc_saved_r = self.last_output_r;
                }
            }
            self.last_process_time = Some(now);
        }

        // Diagnostic: log input boundary on first 3 frames of fresh engine.
        // If input itself has a discontinuity, the pop is from the system audio
        // graph switch, not from our DSP processing.
        if self.process_frame_count <= 3 && ch >= 2 && frames > 0 {
            apo_log!("DIAG frame={}: input_first=({:.10},{:.10}) last_out=({:.6},{:.6})",
                self.process_frame_count,
                input[0], input[1],
                self.last_output_l, self.last_output_r);
        }

        // ═══ Zero-input protection (startup guard) ═══
        // Windows sends all-zero or near-zero blocks during audio graph
        // reconfiguration (UAC, YY Voice, etc.). Don't feed zeros to engines
        // — it dilutes the OLA buffer that was carefully primed. Instead:
        // - All near-zero: smoothstep fadeout from last_output → 0, skip engine
        // - Partial-zero (leading near-zeros): fill zeros with continuation from
        //   last_output, then crossfade output for smooth transition
        //
        // IMPORTANT: Use near-zero threshold (1e-6) instead of exact zero.
        // Windows may send denormalized floats that display as 0.000000
        // but are not exactly 0.0 in IEEE 754.
        const NEAR_ZERO: f32 = 1e-6;
        if self.process_frame_count <= Self::STARTUP_GUARD_FRAMES {
            let mut all_near_zero = true;
            for i in 0..copy_len {
                if input[i].abs() > NEAR_ZERO {
                    all_near_zero = false;
                    break;
                }
            }
            if all_near_zero {
                // All-zero block: smoothstep fadeout from last_output → 0
                if self.last_output_l.abs() > 1e-6 || self.last_output_r.abs() > 1e-6 {
                    if ch >= 2 && frames > 0 {
                        for f in 0..frames {
                            let t = 1.0 - ((f as f32 + 1.0) / frames as f32);
                            let smooth = t * t * (3.0 - 2.0 * t);
                            output[f * 2] = self.last_output_l * smooth;
                            output[f * 2 + 1] = self.last_output_r * smooth;
                        }
                    } else if frames > 0 {
                        for f in 0..frames {
                            let t = 1.0 - ((f as f32 + 1.0) / frames as f32);
                            let smooth = t * t * (3.0 - 2.0 * t);
                            output[f] = self.last_output_l * smooth;
                        }
                    }
                } else {
                    output[..copy_len].fill(0.0);
                }
                apo_log!("ZERO_INPUT_SKIP: frame={}, fadeout from ({:.6},{:.6}), preserved OLA state",
                    self.process_frame_count, self.last_output_l, self.last_output_r);
                self.last_output_l = 0.0;
                self.last_output_r = 0.0;
                self.zero_blocks_count += 1;
                LAST_OUTPUT_L_BITS.store(0f32.to_bits(), Ordering::Relaxed);
                LAST_OUTPUT_R_BITS.store(0f32.to_bits(), Ordering::Relaxed);
                LAST_OUTPUT_VALID.store(true, Ordering::Release);
                return;
            }

            // Partial-zero: detect leading near-zeros in input
            if ch >= 2 && frames > 0 {
                let mut nz_start = 0usize;
                for f in 0..frames {
                    if input[f * 2].abs() > NEAR_ZERO || input[f * 2 + 1].abs() > NEAR_ZERO {
                        nz_start = f;
                        break;
                    }
                    if f == frames - 1 {
                        nz_start = frames; // shouldn't happen (all_near_zero handled above)
                    }
                }
                if nz_start > 0 && nz_start < frames {
                    // Fill leading zeros with smooth continuation from last_output
                    // so engine sees continuous signal (no OLA dilution)
                    for f in 0..nz_start {
                        let t = f as f32 / nz_start as f32;
                        let smooth = t * t * (3.0 - 2.0 * t);
                        self.channel_buf_l[f] = self.last_output_l * (1.0 - smooth) + input[nz_start * 2] * smooth;
                        self.channel_buf_r[f] = self.last_output_r * (1.0 - smooth) + input[nz_start * 2 + 1] * smooth;
                    }
                    // Copy the rest normally
                    for f in nz_start..frames {
                        self.channel_buf_l[f] = input[f * 2];
                        self.channel_buf_r[f] = input[f * 2 + 1];
                    }
                    // Save dry copies
                    self.dry_lr_saved[..frames].copy_from_slice(&self.channel_buf_l[..frames]);
                    self.dry_lr_saved[frames..frames * 2].copy_from_slice(&self.channel_buf_r[..frames]);

                    // Process through engines with filled-in input
                    if let Some(ref mut engine) = self.engine_l {
                        engine.context_mut().cross_channel = self.cross_channel_prev;
                        engine.process(&mut self.channel_buf_l[..frames]);
                    }
                    if let Some(ref mut engine) = self.engine_r {
                        engine.context_mut().cross_channel = self.cross_channel_prev;
                        engine.process(&mut self.channel_buf_r[..frames]);
                    }
                    let cc = CrossChannelContext::from_lr(
                        &self.dry_lr_saved[..frames],
                        &self.dry_lr_saved[frames..frames * 2],
                    );
                    self.cross_channel_prev = Some(cc);

                    // Crossfade output over ENTIRE block from last_output to engine output.
                    // Not just the leading-zero region — when nz_start is small (e.g., 1),
                    // crossfading only 1 sample creates a 0.2+ jump that's audible as a click.
                    // Full-block crossfade spreads the transition over 528 samples (11ms).
                    let saved_l = self.last_output_l;
                    let saved_r = self.last_output_r;
                    for f in 0..frames {
                        let t = (f as f32 + 1.0) / frames as f32;
                        let smooth = t * t * (3.0 - 2.0 * t);
                        self.channel_buf_l[f] = saved_l * (1.0 - smooth) + self.channel_buf_l[f] * smooth;
                        self.channel_buf_r[f] = saved_r * (1.0 - smooth) + self.channel_buf_r[f] * smooth;
                    }

                    // Write interleaved output
                    if enabled {
                        for f in 0..frames {
                            output[f * 2] = self.channel_buf_l[f];
                            output[f * 2 + 1] = self.channel_buf_r[f];
                        }
                    } else {
                        output[..copy_len].copy_from_slice(&input[..copy_len]);
                    }

                    self.last_output_l = self.channel_buf_l[frames - 1];
                    self.last_output_r = self.channel_buf_r[frames - 1];
                    LAST_OUTPUT_L_BITS.store(self.last_output_l.to_bits(), Ordering::Relaxed);
                    LAST_OUTPUT_R_BITS.store(self.last_output_r.to_bits(), Ordering::Relaxed);
                    LAST_OUTPUT_VALID.store(true, Ordering::Release);

                    apo_log!("PARTIAL_ZERO_FILL: frame={}, nz_start={}, crossfaded from ({:.6},{:.6})",
                        self.process_frame_count, nz_start, saved_l, saved_r);
                    return;
                }
            }
        }

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

        // ═══ Post-zero crossfade ═══
        // After zero-input blocks (fadeout to silence), the first real-audio block
        // needs a smooth fade-in. Without this, the transition from 0 → engine output
        // creates a boundary discontinuity. Crossfade from last_output (≈0) into
        // engine output over the block using smoothstep.
        // Always clear zero_blocks_count (even when disabled) to prevent stale
        // crossfade triggering hundreds of frames later when enabled changes.
        if self.zero_blocks_count > 0 {
            self.zero_blocks_count = 0;
            let saved_l = self.last_output_l;
            let saved_r = self.last_output_r;
            if ch >= 2 {
                for f in 0..frames {
                    let t = (f as f32 + 1.0) / frames as f32;
                    let smooth = t * t * (3.0 - 2.0 * t);
                    output[f * 2] = saved_l * (1.0 - smooth) + output[f * 2] * smooth;
                    output[f * 2 + 1] = saved_r * (1.0 - smooth) + output[f * 2 + 1] * smooth;
                }
            } else {
                for f in 0..frames {
                    let t = (f as f32 + 1.0) / frames as f32;
                    let smooth = t * t * (3.0 - 2.0 * t);
                    output[f] = saved_l * (1.0 - smooth) + output[f] * smooth;
                }
            }
            apo_log!("POST_ZERO_CROSSFADE: frame={}, from ({:.6},{:.6})",
                self.process_frame_count, saved_l, saved_r);
        }

        // ═══ DPC spike recovery: mute ═══
        // After a detected DPC latency spike, mute the block. The engine output
        // after a spike contains stale OLA data that sounds robotic/mechanical.
        // A brief silence is far less noticeable than corrupted audio.
        // The click gate will then handle smooth fade-in on the next block.
        if self.dpc_spike_recovery {
            self.dpc_spike_recovery = false;
            output[..copy_len].fill(0.0);
            self.gap_mute_active = true;
            self.process_frame_count = 0;
            apo_log!("DPC_RECOVERY: muted block, last_out=({:.6},{:.6})",
                self.dpc_saved_l, self.dpc_saved_r);
        }

        // ═══ Click Gate (universal output safety) ═══
        // After all output is written, check for boundary discontinuities.
        // If detected, zero the block and activate gap_mute for smooth recovery.
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
            const CLICK_RATIO: f32 = 2.0;
            const CLICK_FLOOR: f32 = 0.05;
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
        // Only store every PRIME_STORE_INTERVAL frames to avoid try_lock overhead
        // on every RT callback. The lock is non-blocking (try_lock), but even the
        // atomic CAS in try_lock is unnecessary on most frames.
        const PRIME_STORE_INTERVAL: u64 = 16;
        if self.process_frame_count > 0 && self.process_frame_count % PRIME_STORE_INTERVAL == 0 {
            if let Ok(mut guard) = STORED_PRIME.try_lock() {
                if let Some(ref mut prime) = *guard {
                    // Only write if already initialized (init happens in lock_for_process)
                    if prime.sample_rate == self.sample_rate && prime.frame_size == self.frame_size {
                        let idx = prime.cursor % Self::PRIME_FRAMES;
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
                // NOTE: if guard is None, skip — allocation happens in lock_for_process
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
