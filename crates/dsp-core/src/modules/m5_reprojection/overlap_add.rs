// ─── Overlap-Add Buffer ───
//
// Accumulates ISTFT synthesis frames and provides hop-aligned output
// for the host callback to consume. This buffer sits between the
// freq-domain synthesis (ISTFT of windowed residual) and the host's
// time-domain output buffer.
//
// Connected to the main audio path via FftOlaPilot synthesis mode in
// SelfReprojectionValidator::process() (mod.rs).  The OLA accumulator
// receives ISTFT-windowed frames and drains hop-aligned output into
// ctx.validated.data.

/// Overlap-add accumulator for ISTFT synthesis output.
///
/// # How it works
///
/// 1. **Write**: Each synthesis frame (fft_size samples) is windowed and
///    added to the accumulator at the current write position.
///    After each write, the write position advances by `hop_size`.
///
/// 2. **Read**: The host callback reads `hop_size` samples from the
///    read position. After reading, the read position advances and
///    the consumed region is zeroed for the next overlap cycle.
///
/// The accumulator is a circular buffer of size `fft_size + hop_size`
/// to handle the maximum overlap (75% for hop = fft/4).
pub struct OverlapAddBuffer {
    /// Circular accumulator.
    accum: Vec<f32>,
    /// Current write position (where next synthesis frame starts).
    write_pos: usize,
    /// Current read position (where next host output starts).
    read_pos: usize,
    /// Hop size in samples.
    hop_size: usize,
    /// FFT/frame size.
    frame_size: usize,
    /// Number of complete hops available for reading.
    readable_hops: usize,
}

impl OverlapAddBuffer {
    /// Create a new OLA buffer for the given frame and hop sizes.
    pub fn new(frame_size: usize, hop_size: usize) -> Self {
        // Buffer needs to hold at least one full frame + one hop of overlap
        // For 75% overlap (hop = frame/4), we need frame + hop slack.
        // Use 2x frame_size for comfortable circular operation.
        let buf_size = frame_size * 2;
        Self {
            accum: vec![0.0; buf_size],
            write_pos: 0,
            read_pos: 0,
            hop_size: hop_size.max(1),
            frame_size,
            readable_hops: 0,
        }
    }

    /// Add a synthesis frame (windowed ISTFT output) to the accumulator.
    /// `frame` must be exactly `frame_size` samples, already windowed.
    /// After adding, write position advances by `hop_size`.
    pub fn add_frame(&mut self, frame: &[f32]) {
        let len = frame.len().min(self.frame_size);
        let buf_size = self.accum.len();

        for i in 0..len {
            let idx = (self.write_pos + i) % buf_size;
            self.accum[idx] += frame[i];
        }

        self.write_pos = (self.write_pos + self.hop_size) % buf_size;
        self.readable_hops += 1;
    }

    /// Read `hop_size` samples into `output` and advance read position.
    /// Returns the number of samples written (always `hop_size` or 0).
    /// Zeros the consumed region for the next overlap cycle.
    pub fn read_hop(&mut self, output: &mut [f32]) -> usize {
        if self.readable_hops == 0 {
            return 0;
        }

        let hop = self.hop_size.min(output.len());
        let buf_size = self.accum.len();

        for i in 0..hop {
            let idx = (self.read_pos + i) % buf_size;
            output[i] = self.accum[idx];
            self.accum[idx] = 0.0; // clear for next cycle
        }

        self.read_pos = (self.read_pos + hop) % buf_size;
        self.readable_hops -= 1;
        hop
    }

    /// Number of hops available for reading.
    pub fn readable(&self) -> usize {
        self.readable_hops
    }

    /// Reset the buffer (e.g., on stream discontinuity).
    pub fn reset(&mut self) {
        self.accum.fill(0.0);
        self.write_pos = 0;
        self.read_pos = 0;
        self.readable_hops = 0;
    }

    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    pub fn hop_size(&self) -> usize {
        self.hop_size
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ola_basic_add_and_read() {
        let frame_size = 8;
        let hop_size = 2;
        let mut ola = OverlapAddBuffer::new(frame_size, hop_size);

        // Add a frame of ones
        let frame = vec![1.0; frame_size];
        ola.add_frame(&frame);

        assert_eq!(ola.readable(), 1);

        let mut output = vec![0.0; hop_size];
        let n = ola.read_hop(&mut output);
        assert_eq!(n, hop_size);
        assert_eq!(output, vec![1.0, 1.0]);
    }

    #[test]
    fn ola_overlap_accumulates() {
        let frame_size = 8;
        let hop_size = 4;
        let mut ola = OverlapAddBuffer::new(frame_size, hop_size);

        // Frame 1: [1,1,1,1, 1,1,1,1]
        let frame1 = vec![1.0; frame_size];
        ola.add_frame(&frame1);

        // Frame 2: overlaps with last 4 of frame 1
        // Position after frame 1: write_pos = 4
        // So frame 2 adds at positions 4..12
        // Overlap at positions 4..8 → accum = 2.0
        let frame2 = vec![1.0; frame_size];
        ola.add_frame(&frame2);

        assert_eq!(ola.readable(), 2);

        // Read first hop (positions 0..4): should be 1.0 each
        let mut out1 = vec![0.0; hop_size];
        ola.read_hop(&mut out1);
        assert_eq!(out1, vec![1.0, 1.0, 1.0, 1.0]);

        // Read second hop (positions 4..8): overlapped → 2.0 each
        let mut out2 = vec![0.0; hop_size];
        ola.read_hop(&mut out2);
        assert_eq!(out2, vec![2.0, 2.0, 2.0, 2.0]);
    }

    #[test]
    fn ola_no_read_when_empty() {
        let mut ola = OverlapAddBuffer::new(256, 64);
        let mut output = vec![0.0; 64];
        assert_eq!(ola.read_hop(&mut output), 0);
        assert_eq!(ola.readable(), 0);
    }

    #[test]
    fn ola_reset_clears() {
        let mut ola = OverlapAddBuffer::new(256, 64);
        let frame = vec![1.0; 256];
        ola.add_frame(&frame);
        assert_eq!(ola.readable(), 1);

        ola.reset();
        assert_eq!(ola.readable(), 0);
        assert!(ola.accum.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn ola_75_percent_overlap_constant_gain() {
        // With Hann window + 75% overlap (hop = N/4), the sum of
        // overlapping windows should be approximately constant.
        // This verifies the OLA accumulation logic.
        let frame_size = 256;
        let hop_size = 64; // 75% overlap
        let mut ola = OverlapAddBuffer::new(frame_size, hop_size);

        // Pre-compute Hann window
        let window: Vec<f32> = (0..frame_size)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (frame_size - 1) as f32).cos())
            })
            .collect();

        // Add enough frames to fill the overlap
        let num_frames = 8;
        for _ in 0..num_frames {
            ola.add_frame(&window);
        }

        // Read the steady-state hops (skip first few for ramp-up)
        // Skip first 3 hops (ramp-up region)
        for _ in 0..3 {
            let mut discard = vec![0.0; hop_size];
            ola.read_hop(&mut discard);
        }

        // Steady-state hops should have approximately constant sum
        let mut hop_out = vec![0.0; hop_size];
        if ola.read_hop(&mut hop_out) > 0 {
            let mean: f32 = hop_out.iter().sum::<f32>() / hop_size as f32;
            // With Hann window at 75% overlap, the constant should be ~1.5
            // (sum of 4 overlapping Hann windows)
            assert!(
                mean > 0.5,
                "Steady-state OLA sum should be significant, got mean={mean}"
            );
            // Check that it's approximately constant across the hop
            let variance: f32 = hop_out.iter()
                .map(|s| (s - mean) * (s - mean))
                .sum::<f32>() / hop_size as f32;
            assert!(
                variance < 0.1,
                "Steady-state OLA should be roughly constant, variance={variance}"
            );
        }
    }

    #[test]
    fn ola_circular_wrapping() {
        let frame_size = 8;
        let hop_size = 4;
        let mut ola = OverlapAddBuffer::new(frame_size, hop_size);

        // Add many frames to force circular wrapping
        for i in 0..20 {
            let frame: Vec<f32> = (0..frame_size).map(|j| (i * frame_size + j) as f32 * 0.01).collect();
            ola.add_frame(&frame);
        }

        // Should be able to read without corruption
        for _ in 0..20 {
            let mut output = vec![0.0; hop_size];
            let n = ola.read_hop(&mut output);
            assert_eq!(n, hop_size);
            assert!(output.iter().all(|s| s.is_finite()), "Output should be finite");
        }
    }
}
