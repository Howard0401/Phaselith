/// Compute declip residual using cubic Hermite back-projection.
///
/// Instead of modifying the signal directly, computes the difference
/// between the clipped signal and the estimated original.
pub fn compute_declip_residual(
    samples: &[f32],
    clipping_severity: f32,
    residual: &mut [f32],
) {
    compute_declip_residual_scaled(samples, clipping_severity, 1.0, 0.5, residual);
}

/// Extended declip with dynamics and transient scaling.
/// - `dynamics`: scales overall declip contribution (0.0-1.0)
/// - `transient`: controls peak estimation aggressiveness (0.0-1.0)
pub fn compute_declip_residual_scaled(
    samples: &[f32],
    clipping_severity: f32,
    dynamics: f32,
    transient: f32,
    residual: &mut [f32],
) {
    let threshold = 0.99;
    let len = samples.len().min(residual.len());
    let strength = clipping_severity.clamp(0.0, 1.0) * dynamics;

    // Peak estimation scaling: transient 0.0 → 0.15 (conservative), 1.0 → 0.35 (aggressive)
    let peak_scale = 0.15 + transient * 0.20;

    let mut clip_start: Option<usize> = None;

    for i in 0..len {
        let is_clipped = samples[i].abs() >= threshold;

        match (is_clipped, clip_start) {
            (true, None) => {
                clip_start = Some(i);
            }
            (false, Some(start)) => {
                compute_region_residual_inner(
                    samples, start, i, threshold, strength, peak_scale, residual,
                );
                clip_start = None;
            }
            _ => {}
        }
    }
}

fn compute_region_residual_inner(
    samples: &[f32],
    start: usize,
    end: usize,
    threshold: f32,
    strength: f32,
    peak_scale: f32,
    residual: &mut [f32],
) {
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

    let estimated_peak =
        threshold + (slope_before.abs() + slope_after.abs()) * 0.5 * (end - start) as f32 * peak_scale;
    let peak = estimated_peak.min(threshold * 1.5);

    let region_len = (end - start).max(1);
    for i in 0..region_len {
        if start + i >= residual.len() {
            break;
        }
        let t = i as f32 / region_len as f32;
        let parabola = peak * 4.0 * t * (1.0 - t);
        let original_estimate = sign * parabola.min(peak);
        // Residual = estimated original - current clipped value
        residual[start + i] = (original_estimate - samples[start + i]) * strength;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declip_residual_nonzero_for_clipped() {
        let mut samples: Vec<f32> = (0..128)
            .map(|i| 1.5 * (2.0 * std::f32::consts::PI * i as f32 / 32.0).sin())
            .collect();
        for s in &mut samples {
            *s = s.clamp(-0.99, 0.99);
        }

        let mut residual = vec![0.0; 128];
        compute_declip_residual(&samples, 0.5, &mut residual);

        let total: f32 = residual.iter().map(|r| r.abs()).sum();
        assert!(total > 0.0, "Residual should be non-zero for clipped signal");
    }

    #[test]
    fn declip_residual_zero_for_clean() {
        let samples: Vec<f32> = (0..128)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * i as f32 / 32.0).sin())
            .collect();

        let mut residual = vec![0.0; 128];
        compute_declip_residual(&samples, 0.5, &mut residual);

        let total: f32 = residual.iter().map(|r| r.abs()).sum();
        assert_eq!(total, 0.0, "Residual should be zero for clean signal");
    }
}
