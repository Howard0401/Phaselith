use crate::config::DspConfig;
use crate::types::{DegradationProfile, HarmonicMap};

/// Shared mutable context passed through the 6-stage pipeline.
/// Earlier stages write fields that later stages read.
///
/// All fields are plain data — no allocations happen here during process().
pub struct StageContext {
    pub config: DspConfig,
    pub sample_rate: u32,
    pub channels: u16,
    /// Written by Stage 0, read by Stage 1-5.
    pub degradation: DegradationProfile,
    /// Written by Stage 2, read by Stage 3-5.
    pub harmonic_map: HarmonicMap,
    /// FFT size currently in use (varies by quality preset).
    pub fft_size: usize,
}

impl StageContext {
    pub fn new(sample_rate: u32, channels: u16, config: DspConfig) -> Self {
        let fft_size = config.quality_mode.fft_size();
        Self {
            config,
            sample_rate,
            channels,
            degradation: DegradationProfile::default(),
            harmonic_map: HarmonicMap::default(),
            fft_size,
        }
    }

    /// Frequency resolution: Hz per FFT bin.
    pub fn bin_to_freq(&self) -> f32 {
        self.sample_rate as f32 / self.fft_size as f32
    }

    /// Convert frequency to FFT bin index.
    pub fn freq_to_bin(&self, freq: f32) -> usize {
        (freq / self.bin_to_freq()) as usize
    }
}

/// The core trait for every DSP processing stage.
///
/// Each of the 6 stages implements this trait. The pipeline calls them in order.
///
/// # Real-time safety contract
///
/// Implementors MUST obey these rules in `process()`:
/// - **No heap allocation** (no Vec::push, no Box::new, no String)
/// - **No mutex or lock** (use atomics if needed)
/// - **No I/O** (no file, no network, no logging)
/// - **No panic** (use checked arithmetic, bounds checking at init time)
/// - **No syscalls** (no Win32 API, no libc calls)
/// - **Constant time** (avoid input-dependent branching that affects timing)
///
/// All scratch memory MUST be pre-allocated in `init()`.
pub trait DspStage: Send + Sync {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;

    /// Pre-allocate scratch buffers for the given max frame size.
    /// Called once at pipeline construction. MAY allocate.
    fn init(&mut self, max_frame_size: usize, sample_rate: u32);

    /// Process audio in-place. MUST be zero-alloc, real-time safe.
    /// Reads/writes `ctx` fields as appropriate for this stage.
    fn process(&mut self, samples: &mut [f32], ctx: &mut StageContext);

    /// Reset internal state (e.g., on stream discontinuity).
    fn reset(&mut self);
}

/// A no-op stage that passes audio through unchanged.
/// Used as default for disabled stages and in tests.
pub struct NoOpStage {
    stage_name: &'static str,
}

impl NoOpStage {
    pub fn new(name: &'static str) -> Self {
        Self { stage_name: name }
    }
}

impl DspStage for NoOpStage {
    fn name(&self) -> &'static str {
        self.stage_name
    }

    fn init(&mut self, _max_frame_size: usize, _sample_rate: u32) {}

    fn process(&mut self, _samples: &mut [f32], _ctx: &mut StageContext) {
        // Pass-through: do nothing
    }

    fn reset(&mut self) {}
}
