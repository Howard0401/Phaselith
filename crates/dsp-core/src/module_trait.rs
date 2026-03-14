use crate::config::EngineConfig;
use crate::frame::{FrameParams, SynthesisMode};

/// Shared mutable context passed through the M0-M7 CIRRUS pipeline.
/// Earlier modules write fields that later modules read.
///
/// All scratch buffers are pre-allocated in each module's `init()`.
/// No allocations happen during `process()`.
pub struct ProcessContext {
    pub config: EngineConfig,
    pub sample_rate: u32,
    pub channels: u16,
    /// Current host callback index (monotonically increasing per process() call).
    pub frame_index: u64,
    /// Frame/hop runtime parameters (immutable during processing).
    pub frame_params: FrameParams,
    /// Analysis frame index — increments on hop boundaries, not on every host callback.
    /// Written by M0 FrameOrchestrator when enough samples accumulate for a new hop.
    pub analysis_frame_index: u64,
    /// Number of hops that completed in the current host callback.
    /// Written by M0. Downstream modules use this to know if new analysis is available.
    pub hops_this_block: usize,
    /// Synthesis mode — controls M5 freq→time conversion path.
    pub synthesis_mode: SynthesisMode,
    /// Written by M1: damage posterior estimation.
    pub damage: crate::types::DamagePosterior,
    /// Written by M2: tri-lattice STFT analysis.
    pub lattice: crate::types::TriLattice,
    /// Written by M3: structured field decomposition.
    pub fields: crate::types::StructuredFields,
    /// Written by M4: inverse residual candidate (freq-domain only).
    pub residual: crate::types::ResidualCandidate,
    /// Written by M4: time-domain residual candidate (per-sample, from declip).
    /// Separate from freq-domain residual to avoid domain mixing.
    /// Sized to sample count, not bin count.
    pub time_candidate: Vec<f32>,
    /// Cross-channel stereo context from previous frame (symmetric one-frame delay).
    /// None in mono mode or on first frame. Used as gate/bias for side recovery.
    pub cross_channel: Option<crate::types::CrossChannelContext>,
    /// Written by M5: validated residual after self-reprojection.
    pub validated: crate::types::ValidatedResidual,
    /// Dry signal copy for M6 safety mixing.
    pub dry_buffer: Vec<f32>,
}

impl ProcessContext {
    pub fn new(sample_rate: u32, channels: u16, config: EngineConfig) -> Self {
        let frame_params = FrameParams::new(128, sample_rate, config.quality_mode);
        Self {
            config,
            sample_rate,
            channels,
            frame_index: 0,
            frame_params,
            analysis_frame_index: 0,
            hops_this_block: 0,
            synthesis_mode: SynthesisMode::default(),
            damage: crate::types::DamagePosterior::default(),
            lattice: crate::types::TriLattice::default(),
            fields: crate::types::StructuredFields::default(),
            residual: crate::types::ResidualCandidate::default(),
            time_candidate: Vec::new(),
            cross_channel: None,
            validated: crate::types::ValidatedResidual::default(),
            dry_buffer: Vec::new(),
        }
    }

    /// Frequency resolution: Hz per FFT bin at a given FFT size.
    pub fn bin_to_freq(&self, fft_size: usize) -> f32 {
        self.sample_rate as f32 / fft_size as f32
    }

    /// Convert frequency to FFT bin index at a given FFT size.
    pub fn freq_to_bin(&self, freq: f32, fft_size: usize) -> usize {
        (freq / self.bin_to_freq(fft_size)) as usize
    }
}

/// The core trait for every CIRRUS processing module (M0-M7).
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
pub trait CirrusModule: Send + Sync {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;

    /// Pre-allocate scratch buffers. Called once at engine construction. MAY allocate.
    fn init(&mut self, max_frame_size: usize, sample_rate: u32);

    /// Process audio in-place. MUST be zero-alloc, real-time safe.
    /// Reads/writes `ctx` fields as appropriate for this module.
    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext);

    /// Reset internal state (e.g., on stream discontinuity).
    fn reset(&mut self);
}

/// A no-op module that passes audio through unchanged.
/// Used as default for disabled modules and in tests.
pub struct NoOpModule {
    module_name: &'static str,
}

impl NoOpModule {
    pub fn new(name: &'static str) -> Self {
        Self { module_name: name }
    }
}

impl CirrusModule for NoOpModule {
    fn name(&self) -> &'static str {
        self.module_name
    }

    fn init(&mut self, _max_frame_size: usize, _sample_rate: u32) {}

    fn process(&mut self, _samples: &mut [f32], _ctx: &mut ProcessContext) {}

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;

    #[test]
    fn noop_module_passes_through() {
        let mut module = NoOpModule::new("test");
        module.init(1024, 48000);
        let mut samples = vec![1.0, 2.0, 3.0];
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        module.process(&mut samples, &mut ctx);
        assert_eq!(samples, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn noop_module_has_name() {
        let module = NoOpModule::new("test-noop");
        assert_eq!(module.name(), "test-noop");
    }

    #[test]
    fn process_context_freq_conversion() {
        let ctx = ProcessContext::new(48000, 2, EngineConfig::default());
        let bin_freq = ctx.bin_to_freq(1024);
        assert!((bin_freq - 46.875).abs() < 0.01); // 48000/1024
        let bin = ctx.freq_to_bin(1000.0, 1024);
        assert_eq!(bin, 21); // 1000/46.875 ≈ 21
    }
}
