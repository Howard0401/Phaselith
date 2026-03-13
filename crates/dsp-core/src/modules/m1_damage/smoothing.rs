use crate::types::GaussianEstimate;

const MAX_CHANNELS: usize = 8;
const EMA_ALPHA: f32 = 0.3; // smoothing factor: higher = more responsive

/// Temporal smoother using exponential moving average (EMA).
/// Maintains separate smoothed state for each damage dimension.
pub struct TemporalSmoother {
    states: [GaussianEstimate; MAX_CHANNELS],
    initialized: [bool; MAX_CHANNELS],
}

impl TemporalSmoother {
    pub fn new() -> Self {
        Self {
            states: [GaussianEstimate::default(); MAX_CHANNELS],
            initialized: [false; MAX_CHANNELS],
        }
    }

    /// Smooth a new estimate for the given channel index.
    /// Returns the smoothed estimate.
    pub fn smooth_estimate(&mut self, channel: usize, raw: GaussianEstimate) -> GaussianEstimate {
        if channel >= MAX_CHANNELS {
            return raw;
        }

        if !self.initialized[channel] {
            self.states[channel] = raw;
            self.initialized[channel] = true;
            return raw;
        }

        let prev = &self.states[channel];
        let smoothed = GaussianEstimate::new(
            prev.mean * (1.0 - EMA_ALPHA) + raw.mean * EMA_ALPHA,
            prev.variance * (1.0 - EMA_ALPHA) + raw.variance * EMA_ALPHA,
        );

        self.states[channel] = smoothed;
        smoothed
    }

    pub fn reset(&mut self) {
        self.states = [GaussianEstimate::default(); MAX_CHANNELS];
        self.initialized = [false; MAX_CHANNELS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoother_first_value_is_raw() {
        let mut smoother = TemporalSmoother::new();
        let raw = GaussianEstimate::new(15000.0, 100.0);
        let result = smoother.smooth_estimate(0, raw);
        assert_eq!(result.mean, 15000.0);
        assert_eq!(result.variance, 100.0);
    }

    #[test]
    fn smoother_converges() {
        let mut smoother = TemporalSmoother::new();

        // Initial value far from target
        smoother.smooth_estimate(0, GaussianEstimate::new(0.0, 1.0));

        // Feed constant target
        let target = 100.0;
        let mut last = 0.0f32;
        for _ in 0..100 {
            let result = smoother.smooth_estimate(0, GaussianEstimate::new(target, 0.1));
            last = result.mean;
        }

        assert!(
            (last - target).abs() < 1.0,
            "Should converge to {}, got {}",
            target,
            last
        );
    }

    #[test]
    fn smoother_reset_clears_state() {
        let mut smoother = TemporalSmoother::new();
        smoother.smooth_estimate(0, GaussianEstimate::new(15000.0, 100.0));
        smoother.reset();

        // Next call should be treated as first
        let result = smoother.smooth_estimate(0, GaussianEstimate::new(8000.0, 50.0));
        assert_eq!(result.mean, 8000.0);
    }

    #[test]
    fn smoother_independent_channels() {
        let mut smoother = TemporalSmoother::new();
        smoother.smooth_estimate(0, GaussianEstimate::new(100.0, 1.0));
        smoother.smooth_estimate(1, GaussianEstimate::new(200.0, 1.0));

        let ch0 = smoother.smooth_estimate(0, GaussianEstimate::new(100.0, 1.0));
        let ch1 = smoother.smooth_estimate(1, GaussianEstimate::new(200.0, 1.0));

        assert!((ch0.mean - 100.0).abs() < 1.0);
        assert!((ch1.mean - 200.0).abs() < 1.0);
    }
}
