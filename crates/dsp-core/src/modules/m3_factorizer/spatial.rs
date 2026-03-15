/// Analyze spatial field: mid/side consistency score Q_spatial.
///
/// Computes per-bin spatial consistency from interleaved stereo samples.
pub fn analyze_spatial(samples: &[f32], core_magnitude: &[f32], spatial_field: &mut [f32]) {
    spatial_field.fill(0.0);

    if samples.len() < 2 || core_magnitude.is_empty() {
        return;
    }

    // Compute mid/side energy ratio per block segment
    let mut mid_energy = 0.0f32;
    let mut side_energy = 0.0f32;

    for chunk in samples.chunks(2) {
        if chunk.len() < 2 {
            break;
        }
        let mid = (chunk[0] + chunk[1]) * 0.5;
        let side = (chunk[0] - chunk[1]) * 0.5;
        mid_energy += mid * mid;
        side_energy += side * side;
    }

    if mid_energy < 1e-10 {
        return;
    }

    let side_ratio = side_energy / mid_energy;

    // Map side_ratio to spatial quality score
    // Normal stereo: side_ratio > 0.2 → Q ≈ 1.0
    // Collapsed stereo: side_ratio < 0.05 → Q ≈ 0.0
    let q_spatial = ((side_ratio - 0.05) / 0.15).clamp(0.0, 1.0);

    // Apply uniform spatial field (could be per-band in future)
    for k in 0..spatial_field.len().min(core_magnitude.len()) {
        spatial_field[k] = q_spatial;
    }
}

/// Compute mid/side decomposition from interleaved samples.
/// Returns (mid_samples, side_samples).
pub fn mid_side_decompose(samples: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let num_frames = samples.len() / 2;
    let mut mid = Vec::with_capacity(num_frames);
    let mut side = Vec::with_capacity(num_frames);

    for chunk in samples.chunks(2) {
        if chunk.len() < 2 {
            break;
        }
        mid.push((chunk[0] + chunk[1]) * 0.5);
        side.push((chunk[0] - chunk[1]) * 0.5);
    }

    (mid, side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_mono_gives_low_q() {
        let mut interleaved = Vec::new();
        for i in 0..512 {
            let v = (i as f32 * 0.1).sin();
            interleaved.push(v);
            interleaved.push(v); // identical L and R
        }
        let magnitude = vec![1.0; 100];
        let mut spatial_field = vec![0.0; 100];

        analyze_spatial(&interleaved, &magnitude, &mut spatial_field);

        assert!(
            spatial_field[50] < 0.3,
            "Mono signal should have low Q_spatial, got {}",
            spatial_field[50]
        );
    }

    #[test]
    fn spatial_stereo_gives_high_q() {
        let mut interleaved = Vec::new();
        for i in 0..512 {
            interleaved.push((i as f32 * 0.1).sin());
            interleaved.push((i as f32 * 0.13).sin());
        }
        let magnitude = vec![1.0; 100];
        let mut spatial_field = vec![0.0; 100];

        analyze_spatial(&interleaved, &magnitude, &mut spatial_field);

        assert!(
            spatial_field[50] > 0.5,
            "Stereo signal should have high Q_spatial, got {}",
            spatial_field[50]
        );
    }

    #[test]
    fn mid_side_decompose_correct() {
        let samples = vec![1.0, 0.5, 0.8, 0.2];
        let (mid, side) = mid_side_decompose(&samples);

        assert_eq!(mid.len(), 2);
        assert!((mid[0] - 0.75).abs() < 1e-6); // (1.0 + 0.5) / 2
        assert!((side[0] - 0.25).abs() < 1e-6); // (1.0 - 0.5) / 2
    }
}
