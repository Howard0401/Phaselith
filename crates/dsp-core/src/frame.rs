// ─── Frame / Hop Runtime Contracts ───
//
// These types formalize the relationship between host callback blocks,
// analysis frames, and hop-based processing. Before this module existed,
// the engine implicitly treated each host callback block as an analysis frame
// (zero-padding to FFT size). This module makes frame/hop semantics explicit.

/// Runtime frame parameters derived from EngineConfig at init time.
/// Immutable during processing — only changes on config update + reinit.
#[derive(Debug, Clone, Copy)]
pub struct FrameParams {
    /// Host callback block size (e.g., 128 for browser AudioWorklet).
    pub host_block_size: usize,
    /// Core lattice FFT size (from QualityMode).
    pub core_fft_size: usize,
    /// Micro lattice FFT size.
    pub micro_fft_size: usize,
    /// Air lattice FFT size.
    pub air_fft_size: usize,
    /// Hop size = core_fft_size / 4 (75% overlap).
    pub hop_size: usize,
    /// Sample rate (Hz).
    pub sample_rate: u32,
}

impl FrameParams {
    pub fn new(host_block_size: usize, sample_rate: u32, quality: crate::config::QualityMode) -> Self {
        let core_fft_size = quality.core_fft_size();
        Self {
            host_block_size,
            core_fft_size,
            micro_fft_size: crate::types::MICRO_FFT_SIZE,
            air_fft_size: crate::types::AIR_FFT_SIZE,
            hop_size: quality.hop_size(),
            sample_rate,
        }
    }

    /// Number of host blocks needed to accumulate one hop.
    /// Returns 0 if host_block_size > hop_size (each block triggers multiple hops).
    pub fn blocks_per_hop(&self) -> usize {
        if self.host_block_size == 0 {
            return 0;
        }
        self.hop_size / self.host_block_size
    }

    /// Number of hops per host block.
    /// Returns 0 if hop_size > host_block_size (needs multiple blocks per hop).
    pub fn hops_per_block(&self) -> usize {
        if self.hop_size == 0 {
            return 0;
        }
        self.host_block_size / self.hop_size
    }

    /// Frequency resolution (Hz per bin) for core lattice.
    pub fn core_bin_hz(&self) -> f32 {
        self.sample_rate as f32 / self.core_fft_size as f32
    }

    /// Number of bins in core lattice (fft_size/2 + 1).
    pub fn core_bins(&self) -> usize {
        self.core_fft_size / 2 + 1
    }
}

impl Default for FrameParams {
    fn default() -> Self {
        Self::new(128, 48000, crate::config::QualityMode::Standard)
    }
}

// SynthesisMode is defined in config.rs and re-exported from lib.rs.
// Re-export here for backward compatibility with existing imports.
pub use crate::config::SynthesisMode;

/// Tracks hop-aligned frame accumulation state.
/// Used by M0 to know when a full analysis frame is ready.
#[derive(Debug, Clone)]
pub struct FrameClock {
    /// Samples accumulated since last hop boundary.
    samples_since_hop: usize,
    /// Hop size (cached from FrameParams).
    hop_size: usize,
    /// Total analysis frames produced.
    analysis_frame_count: u64,
}

impl FrameClock {
    pub fn new(hop_size: usize) -> Self {
        Self {
            samples_since_hop: 0,
            hop_size: hop_size.max(1),
            analysis_frame_count: 0,
        }
    }

    /// Feed samples into the clock. Returns the number of hops that
    /// completed during this block (0 if not enough samples yet).
    pub fn advance(&mut self, num_samples: usize) -> usize {
        self.samples_since_hop += num_samples;
        let hops = self.samples_since_hop / self.hop_size;
        if hops > 0 {
            self.samples_since_hop %= self.hop_size;
            self.analysis_frame_count += hops as u64;
        }
        hops
    }

    /// Total analysis frames produced since init/reset.
    pub fn frame_count(&self) -> u64 {
        self.analysis_frame_count
    }

    /// Samples accumulated toward next hop.
    pub fn pending_samples(&self) -> usize {
        self.samples_since_hop
    }

    pub fn reset(&mut self) {
        self.samples_since_hop = 0;
        self.analysis_frame_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::QualityMode;

    #[test]
    fn frame_params_standard() {
        let fp = FrameParams::new(128, 48000, QualityMode::Standard);
        assert_eq!(fp.core_fft_size, 1024);
        assert_eq!(fp.hop_size, 256);
        assert_eq!(fp.blocks_per_hop(), 2); // 256 / 128
        assert_eq!(fp.hops_per_block(), 0); // 128 < 256
        assert_eq!(fp.core_bins(), 513);
        assert!((fp.core_bin_hz() - 46.875).abs() < 0.01);
    }

    #[test]
    fn frame_params_light() {
        let fp = FrameParams::new(128, 48000, QualityMode::Light);
        assert_eq!(fp.core_fft_size, 512);
        assert_eq!(fp.hop_size, 128);
        assert_eq!(fp.blocks_per_hop(), 1);
        assert_eq!(fp.hops_per_block(), 1);
    }

    #[test]
    fn frame_params_large_block() {
        // APO-style: 480-sample block, Standard mode (hop=256)
        let fp = FrameParams::new(480, 48000, QualityMode::Standard);
        assert_eq!(fp.blocks_per_hop(), 0); // 256/480 = 0
        assert_eq!(fp.hops_per_block(), 1); // 480/256 = 1
    }

    #[test]
    fn frame_clock_accumulates() {
        let mut clock = FrameClock::new(256);

        // 128 samples → not enough for one hop
        assert_eq!(clock.advance(128), 0);
        assert_eq!(clock.pending_samples(), 128);

        // 128 more → exactly one hop
        assert_eq!(clock.advance(128), 1);
        assert_eq!(clock.pending_samples(), 0);
        assert_eq!(clock.frame_count(), 1);
    }

    #[test]
    fn frame_clock_multiple_hops() {
        let mut clock = FrameClock::new(128);
        // 512 samples at once → 4 hops
        assert_eq!(clock.advance(512), 4);
        assert_eq!(clock.frame_count(), 4);
        assert_eq!(clock.pending_samples(), 0);
    }

    #[test]
    fn frame_clock_partial_accumulation() {
        let mut clock = FrameClock::new(256);
        assert_eq!(clock.advance(100), 0);
        assert_eq!(clock.advance(100), 0);
        assert_eq!(clock.advance(100), 1); // 300 total, one hop at 256, 44 remaining
        assert_eq!(clock.pending_samples(), 44);
    }

    #[test]
    fn frame_clock_reset() {
        let mut clock = FrameClock::new(256);
        clock.advance(512);
        clock.reset();
        assert_eq!(clock.frame_count(), 0);
        assert_eq!(clock.pending_samples(), 0);
    }

    #[test]
    fn synthesis_mode_default_is_legacy() {
        assert_eq!(SynthesisMode::default(), SynthesisMode::LegacyAdditive);
    }
}
