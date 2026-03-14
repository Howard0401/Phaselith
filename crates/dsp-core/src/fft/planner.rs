use std::collections::HashMap;
use std::sync::Arc;
use rustfft::{Fft, FftPlanner};

/// Shared FFT plan cache — avoids re-planning the same FFT sizes
/// across multiple StftEngine instances (M2 micro/core/air + M5 pilot).
///
/// Each unique (size, direction) pair is planned once and returned as
/// an `Arc<dyn Fft<f32>>`, which is cheap to clone into StftEngine fields.
pub struct SharedFftPlans {
    forward: HashMap<usize, Arc<dyn Fft<f32>>>,
    inverse: HashMap<usize, Arc<dyn Fft<f32>>>,
    planner: FftPlanner<f32>,
}

impl SharedFftPlans {
    pub fn new() -> Self {
        Self {
            forward: HashMap::new(),
            inverse: HashMap::new(),
            planner: FftPlanner::new(),
        }
    }

    /// Get or create a forward FFT plan for the given size.
    pub fn forward(&mut self, size: usize) -> Arc<dyn Fft<f32>> {
        self.forward
            .entry(size)
            .or_insert_with_key(|&sz| self.planner.plan_fft_forward(sz))
            .clone()
    }

    /// Get or create an inverse FFT plan for the given size.
    pub fn inverse(&mut self, size: usize) -> Arc<dyn Fft<f32>> {
        self.inverse
            .entry(size)
            .or_insert_with_key(|&sz| self.planner.plan_fft_inverse(sz))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_plans_returns_same_arc_for_same_size() {
        let mut plans = SharedFftPlans::new();
        let fwd_a = plans.forward(1024);
        let fwd_b = plans.forward(1024);
        // Same Arc → same pointer
        assert!(Arc::ptr_eq(&fwd_a, &fwd_b));
    }

    #[test]
    fn shared_plans_different_sizes_different_arcs() {
        let mut plans = SharedFftPlans::new();
        let fwd_256 = plans.forward(256);
        let fwd_1024 = plans.forward(1024);
        assert!(!Arc::ptr_eq(&fwd_256, &fwd_1024));
    }

    #[test]
    fn shared_plans_forward_inverse_independent() {
        let mut plans = SharedFftPlans::new();
        let fwd = plans.forward(512);
        let inv = plans.inverse(512);
        // Different plans (forward vs inverse)
        assert!(!Arc::ptr_eq(&fwd, &inv));
    }

    #[test]
    fn shared_plans_inverse_returns_same_arc() {
        let mut plans = SharedFftPlans::new();
        let inv_a = plans.inverse(2048);
        let inv_b = plans.inverse(2048);
        assert!(Arc::ptr_eq(&inv_a, &inv_b));
    }
}
