/// Compute soft crossover gain around the cutoff frequency.
///
/// Below cutoff: gain = 0 (preserve original)
/// At cutoff: gain = 0.5
/// Above cutoff: gain → 1.0
///
/// Uses a sigmoid transition for smooth blending.
pub fn crossover_gain(bin: usize, cutoff_bin: usize, transition_width: usize) -> f32 {
    if bin < cutoff_bin.saturating_sub(transition_width) {
        return 0.0;
    }
    if bin > cutoff_bin + transition_width {
        return 1.0;
    }

    let center = cutoff_bin as f32;
    let x = (bin as f32 - center) / transition_width.max(1) as f32;
    // Sigmoid: 1 / (1 + exp(-4x))
    1.0 / (1.0 + (-4.0 * x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossover_zero_far_below() {
        assert_eq!(crossover_gain(10, 100, 10), 0.0);
    }

    #[test]
    fn crossover_one_far_above() {
        assert_eq!(crossover_gain(200, 100, 10), 1.0);
    }

    #[test]
    fn crossover_half_at_center() {
        let gain = crossover_gain(100, 100, 10);
        assert!(
            (gain - 0.5).abs() < 0.01,
            "Should be ~0.5 at center, got {}",
            gain
        );
    }

    #[test]
    fn crossover_monotonic() {
        let cutoff = 100;
        let width = 10;
        let mut prev = 0.0f32;
        for bin in (cutoff - width)..=(cutoff + width) {
            let gain = crossover_gain(bin, cutoff, width);
            assert!(gain >= prev, "Should be monotonically increasing");
            prev = gain;
        }
    }
}
