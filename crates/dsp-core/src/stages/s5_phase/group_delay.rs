use rustfft::num_complex::Complex;

/// Ensure phase coherence for reconstructed frequency bins.
///
/// Extrapolates the group delay trend from below the cutoff
/// into the reconstructed region.
pub fn ensure_phase_coherence(
    spectrum: &mut [Complex<f32>],
    cutoff_bin: usize,
    blend_ratio: f32,
) {
    if cutoff_bin < 3 || cutoff_bin >= spectrum.len() {
        return;
    }

    // Compute phase slope near cutoff (group delay approximation)
    let p1 = spectrum[cutoff_bin - 2].arg();
    let p2 = spectrum[cutoff_bin - 1].arg();
    let phase_slope = unwrap_phase(p2 - p1);
    let ref_phase = spectrum[cutoff_bin.saturating_sub(1)].arg();

    for bin in cutoff_bin..spectrum.len() {
        let current_mag = spectrum[bin].norm();
        if current_mag < 1e-10 {
            continue;
        }

        let expected_phase = ref_phase + phase_slope * (bin - cutoff_bin + 1) as f32;
        let current_phase = spectrum[bin].arg();
        let blended = lerp_angle(current_phase, expected_phase, blend_ratio);

        spectrum[bin] = Complex::from_polar(current_mag, blended);
    }
}

fn unwrap_phase(phase: f32) -> f32 {
    let mut p = phase;
    while p > std::f32::consts::PI {
        p -= 2.0 * std::f32::consts::PI;
    }
    while p < -std::f32::consts::PI {
        p += 2.0 * std::f32::consts::PI;
    }
    p
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let diff = unwrap_phase(b - a);
    unwrap_phase(a + diff * t)
}
