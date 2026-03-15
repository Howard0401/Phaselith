/// Compute harmonic continuation residual above cutoff.
///
/// Extends harmonic series found below the cutoff into the region above it,
/// using the detected ridge structure and decay rate.
pub fn compute_harmonic_extension(
    magnitude: &[f32],
    _phase: &[f32],
    harmonic_field: &[f32],
    cutoff_bin: usize,
    bin_to_freq: f32,
    strength: f32,
    residual: &mut [f32],
) {
    compute_harmonic_extension_styled(
        magnitude,
        _phase,
        harmonic_field,
        cutoff_bin,
        bin_to_freq,
        strength,
        0.5,
        0.4, // default air_brightness & body
        residual,
    );
}

/// Extended harmonic continuation with style parameters.
/// - `air_brightness`: 0.0-1.0, modifies HF decay slope (brighter = shallower rolloff)
/// - `body`: 0.0-1.0, reinforces low-mid harmonics (180-500 Hz)
pub fn compute_harmonic_extension_styled(
    magnitude: &[f32],
    _phase: &[f32],
    harmonic_field: &[f32],
    cutoff_bin: usize,
    bin_to_freq: f32,
    strength: f32,
    air_brightness: f32,
    body: f32,
    residual: &mut [f32],
) {
    let num_bins = residual.len().min(magnitude.len());
    if cutoff_bin >= num_bins || cutoff_bin < 5 {
        // Even if no cutoff detected, still do body reinforcement
        if body > 0.01 && num_bins > 0 {
            apply_body_reinforcement(
                magnitude,
                harmonic_field,
                bin_to_freq,
                body,
                strength,
                num_bins,
                residual,
            );
        }
        return;
    }

    // Estimate average harmonic decay rate below cutoff
    let base_decay = estimate_decay_rate(magnitude, harmonic_field, cutoff_bin);

    // air_brightness modifies decay (decay is negative dB/octave):
    // bright = shallower rolloff = smaller magnitude of negative number
    // 0.0 → ×1.3 (steeper = darker), 1.0 → ×0.7 (shallower = brighter)
    let decay_rate = base_decay * (1.3 - air_brightness * 0.6);

    // Reference level at cutoff
    let ref_level = average_magnitude_near(magnitude, cutoff_bin, 5);
    if ref_level < 1e-10 {
        return;
    }

    // Extend harmonics above cutoff
    let cutoff_freq = cutoff_bin as f32 * bin_to_freq;

    for k in cutoff_bin..num_bins {
        let freq = k as f32 * bin_to_freq;
        if freq > 22000.0 {
            break;
        }

        // Decay based on distance from cutoff
        let octaves_above = (freq / cutoff_freq.max(1.0)).log2();
        let decayed = ref_level * (10.0f32).powf(decay_rate * octaves_above / 20.0);

        // Weight by harmonic field (if harmonic content exists nearby)
        let harmonic_weight = if k < harmonic_field.len() && harmonic_field[k] > 1e-10 {
            1.0
        } else {
            0.3 // small contribution even without harmonics
        };

        residual[k] = decayed * harmonic_weight * strength;
    }

    // Body reinforcement in 180-500 Hz range
    apply_body_reinforcement(
        magnitude,
        harmonic_field,
        bin_to_freq,
        body,
        strength,
        num_bins,
        residual,
    );
}

/// Reinforce existing harmonic energy in the 180-500 Hz range.
/// Works even on high-quality sources by enriching existing harmonic structure.
fn apply_body_reinforcement(
    magnitude: &[f32],
    harmonic_field: &[f32],
    bin_to_freq: f32,
    body: f32,
    strength: f32,
    num_bins: usize,
    residual: &mut [f32],
) {
    if body < 0.01 {
        return;
    }

    let body_lo_bin = (180.0 / bin_to_freq) as usize;
    let body_hi_bin = (500.0 / bin_to_freq).min(num_bins as f32) as usize;
    let body_scale = body * 0.3; // subtle: max 30% reinforcement

    for k in body_lo_bin..body_hi_bin.min(num_bins) {
        if k < harmonic_field.len() && harmonic_field[k] > 1e-10 && k < magnitude.len() {
            // Reinforce existing harmonic energy (not creating new content)
            residual[k] += magnitude[k] * harmonic_field[k] * body_scale * strength;
        }
    }
}

/// Estimate decay rate (dB/octave) from harmonic peaks below cutoff.
fn estimate_decay_rate(magnitude: &[f32], harmonic_field: &[f32], cutoff_bin: usize) -> f32 {
    let mut high_energy = 0.0f32;
    let mut low_energy = 0.0f32;
    let quarter = cutoff_bin / 4;

    // Compare energy in lower vs upper quarter of below-cutoff region
    let h_len = harmonic_field.len().min(cutoff_bin);

    for k in quarter..cutoff_bin / 2 {
        if k < h_len {
            low_energy += magnitude[k] * magnitude[k];
        }
    }
    for k in cutoff_bin / 2..cutoff_bin.min(magnitude.len()) {
        if k < h_len {
            high_energy += magnitude[k] * magnitude[k];
        }
    }

    if low_energy < 1e-10 || high_energy < 1e-10 {
        return -6.0; // default
    }

    let ratio_db = 10.0 * (high_energy / low_energy).log10();
    ratio_db.clamp(-18.0, 0.0) // reasonable range
}

fn average_magnitude_near(magnitude: &[f32], center: usize, radius: usize) -> f32 {
    let start = center.saturating_sub(radius);
    let end = (center + radius).min(magnitude.len());
    if start >= end {
        return 0.0;
    }
    magnitude[start..end].iter().sum::<f32>() / (end - start) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonic_extension_adds_above_cutoff() {
        let num_bins = 513;
        let bin_to_freq = 48000.0 / 1024.0;
        let cutoff_bin = 340; // ~16 kHz

        let mut magnitude = vec![0.0; num_bins];
        let mut harmonic_field = vec![0.0; num_bins];
        let phase = vec![0.0; num_bins];

        // Put energy below cutoff
        for k in 10..cutoff_bin {
            magnitude[k] = 1.0 / (1.0 + k as f32 / 100.0);
            harmonic_field[k] = magnitude[k];
        }

        let mut residual = vec![0.0; num_bins];
        compute_harmonic_extension(
            &magnitude,
            &phase,
            &harmonic_field,
            cutoff_bin,
            bin_to_freq,
            1.0,
            &mut residual,
        );

        let above_energy: f32 = residual[cutoff_bin..].iter().sum();
        assert!(above_energy > 0.0, "Should have extension above cutoff");
    }

    #[test]
    fn harmonic_extension_zero_for_lossless() {
        let num_bins = 513;
        let cutoff_bin = num_bins; // no cutoff
        let mut residual = vec![0.0; num_bins];

        compute_harmonic_extension(
            &vec![1.0; num_bins],
            &vec![0.0; num_bins],
            &vec![0.0; num_bins],
            cutoff_bin,
            46.875,
            1.0,
            &mut residual,
        );

        let total: f32 = residual.iter().sum();
        assert_eq!(total, 0.0, "No extension needed for lossless");
    }

    #[test]
    fn body_reinforcement_adds_low_mid_energy() {
        let num_bins = 513;
        let bin_to_freq = 48000.0 / 1024.0; // ~46.875 Hz/bin

        let mut magnitude = vec![0.0; num_bins];
        let mut harmonic_field = vec![0.0; num_bins];
        let phase = vec![0.0; num_bins];

        // Put harmonic energy in 180-500 Hz range
        let lo = (180.0 / bin_to_freq) as usize;
        let hi = (500.0 / bin_to_freq) as usize;
        for k in lo..hi {
            magnitude[k] = 0.5;
            harmonic_field[k] = 0.5;
        }

        let mut residual = vec![0.0; num_bins];
        compute_harmonic_extension_styled(
            &magnitude,
            &phase,
            &harmonic_field,
            num_bins, // no cutoff (lossless)
            bin_to_freq,
            1.0,
            0.5,
            0.6, // air_brightness, body
            &mut residual,
        );

        // Body reinforcement should add energy in 180-500 Hz
        let body_energy: f32 = residual[lo..hi].iter().sum();
        assert!(
            body_energy > 0.0,
            "Body reinforcement should add low-mid energy, got {}",
            body_energy
        );
    }

    #[test]
    fn air_brightness_affects_decay() {
        let num_bins = 513;
        let bin_to_freq = 48000.0 / 1024.0;
        let cutoff_bin = 340;
        let phase = vec![0.0; num_bins];

        let mut magnitude = vec![0.0; num_bins];
        let harmonic_field = vec![0.0; num_bins];
        for k in 10..cutoff_bin {
            magnitude[k] = 1.0 / (1.0 + k as f32 / 100.0);
        }

        // Dark: air_brightness = 0.0
        let mut residual_dark = vec![0.0; num_bins];
        compute_harmonic_extension_styled(
            &magnitude,
            &phase,
            &harmonic_field,
            cutoff_bin,
            bin_to_freq,
            1.0,
            0.0,
            0.0,
            &mut residual_dark,
        );

        // Bright: air_brightness = 1.0
        let mut residual_bright = vec![0.0; num_bins];
        compute_harmonic_extension_styled(
            &magnitude,
            &phase,
            &harmonic_field,
            cutoff_bin,
            bin_to_freq,
            1.0,
            1.0,
            0.0,
            &mut residual_bright,
        );

        let dark_energy: f32 = residual_dark[cutoff_bin..].iter().sum();
        let bright_energy: f32 = residual_bright[cutoff_bin..].iter().sum();
        // Brighter should have more HF energy (shallower decay)
        assert!(
            bright_energy > dark_energy,
            "Bright ({}) should exceed dark ({})",
            bright_energy,
            dark_energy
        );
    }
}
