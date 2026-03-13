/// Analyze stereo degradation from interleaved L/R samples.
///
/// Joint stereo compression reduces the side channel energy,
/// making the stereo image narrower. This detects that degradation.
///
/// Returns 0.0 (normal stereo) to 1.0 (severe degradation / mono).
pub fn analyze_stereo(left: &[f32], right: &[f32]) -> f32 {
    let mut mid_energy = 0.0f32;
    let mut side_energy = 0.0f32;

    for (l, r) in left.iter().zip(right.iter()) {
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5;
        mid_energy += mid * mid;
        side_energy += side * side;
    }

    if mid_energy < 1e-10 {
        return 0.0;
    }

    let side_ratio = side_energy / mid_energy;

    if side_ratio < 0.1 {
        0.8
    } else if side_ratio < 0.2 {
        0.4
    } else {
        0.0
    }
}

/// Analyze stereo degradation from interleaved samples [L, R, L, R, ...].
pub fn analyze_stereo_interleaved(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }

    let mut mid_energy = 0.0f32;
    let mut side_energy = 0.0f32;
    let mut count = 0;

    for chunk in samples.chunks(2) {
        if chunk.len() < 2 {
            break;
        }
        let l = chunk[0];
        let r = chunk[1];
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.5;
        mid_energy += mid * mid;
        side_energy += side * side;
        count += 1;
    }

    if count == 0 || mid_energy < 1e-10 {
        return 0.0;
    }

    let side_ratio = side_energy / mid_energy;

    if side_ratio < 0.1 {
        0.8
    } else if side_ratio < 0.2 {
        0.4
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_signal_shows_degradation() {
        let mono: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
        let result = analyze_stereo(&mono, &mono);
        assert!(result > 0.5, "Mono signal should show high degradation");
    }

    #[test]
    fn stereo_signal_shows_no_degradation() {
        let left: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
        let right: Vec<f32> = (0..512).map(|i| (i as f32 * 0.13).sin()).collect();
        let result = analyze_stereo(&left, &right);
        assert!(result < 0.5, "Different L/R should show low degradation");
    }
}
