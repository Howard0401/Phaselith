use crate::module_trait::{CirrusModule, ProcessContext};

/// M0: Frame Orchestrator.
///
/// Manages ring buffers, hop/frame alignment, and dry path preservation.
/// Supports three concurrent window sizes for the tri-lattice.
pub struct FrameOrchestrator {
    /// Ring buffer for incoming samples.
    ring_buffer: Vec<f32>,
    /// Write position in ring buffer.
    write_pos: usize,
    /// Total samples received (for hop alignment).
    total_samples: u64,
    /// Hop size (from quality mode).
    hop_size: usize,
    /// Maximum frame size.
    max_frame_size: usize,
    sample_rate: u32,
}

impl FrameOrchestrator {
    pub fn new() -> Self {
        Self {
            ring_buffer: Vec::new(),
            write_pos: 0,
            total_samples: 0,
            hop_size: 256,
            max_frame_size: 0,
            sample_rate: 48000,
        }
    }

    /// Read the last `len` samples from the ring buffer into `output`.
    /// Returns actual number of samples copied.
    pub fn read_last(&self, output: &mut [f32], len: usize) -> usize {
        let ring_len = self.ring_buffer.len();
        if ring_len == 0 || len == 0 {
            return 0;
        }
        let actual_len = len.min(ring_len).min(output.len());
        let start = if self.write_pos >= actual_len {
            self.write_pos - actual_len
        } else {
            ring_len - (actual_len - self.write_pos)
        };

        for i in 0..actual_len {
            let idx = (start + i) % ring_len;
            output[i] = self.ring_buffer[idx];
        }
        actual_len
    }

    /// Current write position.
    pub fn write_position(&self) -> usize {
        self.write_pos
    }

    /// Total samples written since init/reset.
    pub fn total_samples_written(&self) -> u64 {
        self.total_samples
    }
}

impl CirrusModule for FrameOrchestrator {
    fn name(&self) -> &'static str {
        "M0:Orchestrator"
    }

    fn init(&mut self, max_frame_size: usize, sample_rate: u32) {
        self.max_frame_size = max_frame_size;
        self.sample_rate = sample_rate;
        // Ring buffer holds enough for the largest FFT window (air = 2048/4096)
        // plus extra for overlap
        let ring_size = crate::types::AIR_FFT_SIZE * 2;
        self.ring_buffer = vec![0.0; ring_size];
        self.write_pos = 0;
    }

    fn process(&mut self, samples: &mut [f32], ctx: &mut ProcessContext) {
        self.hop_size = ctx.config.quality_mode.hop_size();

        // Write incoming samples into ring buffer
        let ring_len = self.ring_buffer.len();
        for &s in samples.iter() {
            self.ring_buffer[self.write_pos % ring_len] = s;
            self.write_pos = (self.write_pos + 1) % ring_len;
        }
        self.total_samples += samples.len() as u64;
    }

    fn reset(&mut self) {
        self.ring_buffer.fill(0.0);
        self.write_pos = 0;
        self.total_samples = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;

    #[test]
    fn orchestrator_init_creates_ring_buffer() {
        let mut m0 = FrameOrchestrator::new();
        m0.init(1024, 48000);
        assert!(!m0.ring_buffer.is_empty());
    }

    #[test]
    fn orchestrator_writes_to_ring() {
        let mut m0 = FrameOrchestrator::new();
        m0.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        let mut samples = vec![1.0, 2.0, 3.0, 4.0];
        m0.process(&mut samples, &mut ctx);

        assert_eq!(m0.total_samples_written(), 4);
    }

    #[test]
    fn orchestrator_read_last() {
        let mut m0 = FrameOrchestrator::new();
        m0.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        let mut samples: Vec<f32> = (1..=10).map(|x| x as f32).collect();
        m0.process(&mut samples, &mut ctx);

        let mut output = vec![0.0; 4];
        let n = m0.read_last(&mut output, 4);
        assert_eq!(n, 4);
        assert_eq!(output, vec![7.0, 8.0, 9.0, 10.0]);
    }

    #[test]
    fn orchestrator_ring_wraps() {
        let mut m0 = FrameOrchestrator::new();
        m0.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        // Write more than ring size to force wrapping
        let ring_size = m0.ring_buffer.len();
        let mut big_block: Vec<f32> = (0..ring_size + 100)
            .map(|x| x as f32)
            .collect();
        m0.process(&mut big_block, &mut ctx);

        // Should still be able to read the last samples correctly
        let mut output = vec![0.0; 4];
        let n = m0.read_last(&mut output, 4);
        assert_eq!(n, 4);
        let expected_last = (ring_size + 100 - 1) as f32;
        assert_eq!(output[3], expected_last);
    }

    #[test]
    fn orchestrator_reset_clears() {
        let mut m0 = FrameOrchestrator::new();
        m0.init(1024, 48000);
        let mut ctx = ProcessContext::new(48000, 2, EngineConfig::default());

        let mut samples = vec![1.0, 2.0, 3.0];
        m0.process(&mut samples, &mut ctx);
        m0.reset();

        assert_eq!(m0.total_samples_written(), 0);
        assert_eq!(m0.write_position(), 0);
    }
}
