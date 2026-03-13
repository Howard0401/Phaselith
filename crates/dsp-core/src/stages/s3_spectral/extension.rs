use crate::types::HarmonicMap;
use rustfft::num_complex::Complex;

/// Extend harmonics above the cutoff frequency using fitted decay curves.
pub fn extend_harmonics(
    spectrum: &mut [Complex<f32>],
    harmonic_map: &HarmonicMap,
    cutoff: f32,
    bin_to_freq: f32,
    strength: f32,
) {
    for track in &harmonic_map.tracks {
        let f0 = track.freq;
        if f0 < 20.0 {
            continue;
        }

        let max_known_order = (cutoff / f0) as usize;
        let mut n = max_known_order + 1;

        while f0 * n as f32 <= 22000.0 {
            let target_freq = f0 * n as f32;
            let target_bin = (target_freq / bin_to_freq) as usize;

            if target_bin >= spectrum.len() {
                break;
            }

            // Predict magnitude from decay curve
            let base_mag = if let Some(h) = track.harmonics.first() {
                h.magnitude
            } else {
                break;
            };

            let predicted_mag = base_mag * (n as f32).powf(-track.decay_rate);

            // Extrapolate phase from last known harmonics
            let phase = extrapolate_phase(track, n);

            spectrum[target_bin] += Complex::from_polar(predicted_mag * strength, phase);

            n += 1;
        }
    }
}

fn extrapolate_phase(track: &crate::types::FundamentalTrack, target_order: usize) -> f32 {
    let harmonics = &track.harmonics;
    if harmonics.len() < 2 {
        return 0.0;
    }

    let last = harmonics.last().unwrap();
    let second_last = &harmonics[harmonics.len() - 2];
    let phase_step = unwrap_phase(last.phase - second_last.phase);
    let steps = target_order.saturating_sub(last.order);

    unwrap_phase(last.phase + phase_step * steps as f32)
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
