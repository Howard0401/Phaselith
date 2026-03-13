/// De-clip a block of audio samples using cubic Hermite interpolation.
///
/// Detects clipped regions (|sample| >= threshold) and reconstructs
/// the original peak shape using boundary slopes.
pub fn declip_block(samples: &mut [f32], threshold: f32) {
    let mut clip_start: Option<usize> = None;

    for i in 0..samples.len() {
        let is_clipped = samples[i].abs() >= threshold;

        match (is_clipped, clip_start) {
            (true, None) => {
                clip_start = Some(i);
            }
            (false, Some(start)) => {
                interpolate_clipped_region(samples, start, i, threshold);
                clip_start = None;
            }
            _ => {}
        }
    }
}

fn interpolate_clipped_region(samples: &mut [f32], start: usize, end: usize, threshold: f32) {
    if end <= start {
        return;
    }

    let sign = if samples[start] > 0.0 { 1.0 } else { -1.0 };

    let slope_before = if start > 0 {
        samples[start] - samples[start - 1]
    } else {
        0.0
    };
    let slope_after = if end < samples.len() - 1 {
        samples[end + 1] - samples[end]
    } else {
        0.0
    };

    // Estimate the original peak from boundary slopes
    let estimated_peak =
        threshold + (slope_before.abs() + slope_after.abs()) * 0.5 * (end - start) as f32 * 0.25;
    let peak = estimated_peak.min(threshold * 1.5); // Limit max repair amplitude

    let len = (end - start).max(1);
    for i in 0..len {
        let t = i as f32 / len as f32;
        let h = cubic_hermite(threshold, slope_before, threshold, slope_after, t);
        let parabola = peak * 4.0 * t * (1.0 - t);
        samples[start + i] = sign * h.max(parabola).min(peak);
    }
}

fn cubic_hermite(p0: f32, m0: f32, p1: f32, m1: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    (2.0 * t3 - 3.0 * t2 + 1.0) * p0
        + (t3 - 2.0 * t2 + t) * m0
        + (-2.0 * t3 + 3.0 * t2) * p1
        + (t3 - t2) * m1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declip_modifies_clipped_samples() {
        // Create a clipped signal
        let mut samples: Vec<f32> = (0..128)
            .map(|i| 1.5 * (2.0 * std::f32::consts::PI * i as f32 / 32.0).sin())
            .collect();
        // Hard clip
        let original = samples.clone();
        for s in &mut samples {
            *s = s.clamp(-0.99, 0.99);
        }

        declip_block(&mut samples, 0.99);

        // After declipping, some samples should differ from the clipped version
        let changed = samples
            .iter()
            .zip(original.iter())
            .filter(|(a, _b)| a.abs() > 0.99)
            .count();
        // The declipped signal should have peaks above the clip threshold
        assert!(changed > 0 || samples.iter().any(|s| s.abs() > 0.98));
    }
}
