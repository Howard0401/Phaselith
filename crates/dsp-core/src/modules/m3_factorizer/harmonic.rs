/// Detect harmonic ridges in a magnitude spectrum.
///
/// Finds the dominant fundamental frequency and scores how well
/// the spectrum matches a harmonic series at that f0.
///
/// Returns (fundamental_freq, ridge_score) and fills the harmonic field.
pub fn detect_ridges(
    magnitude: &[f32],
    bin_to_freq: f32,
    harmonic_field: &mut [f32],
) -> (Option<f32>, f32) {
    harmonic_field.fill(0.0);

    if magnitude.len() < 10 {
        return (None, 0.0);
    }

    // Find peaks in the spectrum
    let peaks = find_spectral_peaks(magnitude, 10);
    if peaks.is_empty() {
        return (None, 0.0);
    }

    // Try each peak as a potential fundamental
    let mut best_f0 = 0.0f32;
    let mut best_score = 0.0f32;

    for &peak_bin in &peaks {
        let f0 = peak_bin as f32 * bin_to_freq;
        if f0 < 50.0 || f0 > 4000.0 {
            continue;
        }

        let score = ridge_score(magnitude, f0, bin_to_freq);
        if score > best_score {
            best_score = score;
            best_f0 = f0;
        }
    }

    if best_score < 0.3 {
        return (None, 0.0);
    }

    // Fill harmonic field for the best fundamental
    fill_harmonic_field(magnitude, best_f0, bin_to_freq, harmonic_field);

    (Some(best_f0), best_score)
}

/// Score how well the spectrum matches a harmonic series at frequency f0.
/// R(f0, m) = Σ_n w_n · |X(n·f0)|² / Σ_k |X(k)|²
fn ridge_score(magnitude: &[f32], f0: f32, bin_to_freq: f32) -> f32 {
    let total_energy: f32 = magnitude.iter().map(|m| m * m).sum();
    if total_energy < 1e-10 {
        return 0.0;
    }

    let mut harmonic_energy = 0.0f32;
    let max_harmonic = 16;
    let tolerance = 2; // bins

    for n in 1..=max_harmonic {
        let target_bin = ((n as f32 * f0) / bin_to_freq) as usize;
        if target_bin >= magnitude.len() {
            break;
        }

        // Find peak near target bin
        let start = target_bin.saturating_sub(tolerance);
        let end = (target_bin + tolerance).min(magnitude.len() - 1);

        let local_peak = magnitude[start..=end]
            .iter()
            .map(|m| m * m)
            .fold(0.0f32, f32::max);

        let weight = 1.0 / (n as f32).sqrt();
        harmonic_energy += weight * local_peak;
    }

    harmonic_energy / total_energy
}

/// Fill the harmonic field with energy at harmonic locations.
fn fill_harmonic_field(
    magnitude: &[f32],
    f0: f32,
    bin_to_freq: f32,
    harmonic_field: &mut [f32],
) {
    let max_harmonic = 32;
    let tolerance = 2;

    for n in 1..=max_harmonic {
        let target_bin = ((n as f32 * f0) / bin_to_freq) as usize;
        if target_bin >= harmonic_field.len() {
            break;
        }

        let start = target_bin.saturating_sub(tolerance);
        let end = (target_bin + tolerance).min(harmonic_field.len() - 1);

        for k in start..=end {
            if k < magnitude.len() {
                harmonic_field[k] = harmonic_field[k].max(magnitude[k]);
            }
        }
    }
}

/// Find the top N spectral peaks (local maxima).
fn find_spectral_peaks(magnitude: &[f32], max_peaks: usize) -> Vec<usize> {
    let mut peaks: Vec<(usize, f32)> = Vec::new();

    for i in 1..magnitude.len() - 1 {
        if magnitude[i] > magnitude[i - 1] && magnitude[i] > magnitude[i + 1] {
            peaks.push((i, magnitude[i]));
        }
    }

    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    peaks.truncate(max_peaks);
    peaks.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_harmonic_spectrum(f0: f32, bin_to_freq: f32, num_bins: usize) -> Vec<f32> {
        let mut magnitude = vec![0.001; num_bins];
        for n in 1..=10 {
            let bin = ((n as f32 * f0) / bin_to_freq) as usize;
            if bin < num_bins {
                magnitude[bin] = 1.0 / (n as f32);
            }
        }
        magnitude
    }

    #[test]
    fn detects_440hz_harmonic() {
        let bin_to_freq = 48000.0 / 1024.0; // ~46.875 Hz/bin
        let num_bins = 513;
        let magnitude = make_harmonic_spectrum(440.0, bin_to_freq, num_bins);
        let mut harmonic_field = vec![0.0; num_bins];

        let (f0, score) = detect_ridges(&magnitude, bin_to_freq, &mut harmonic_field);

        assert!(f0.is_some(), "Should detect fundamental");
        let detected_f0 = f0.unwrap();
        assert!(
            (detected_f0 - 440.0).abs() < 100.0,
            "Should be near 440 Hz, got {}",
            detected_f0
        );
        assert!(score > 0.3, "Ridge score should be significant, got {}", score);
    }

    #[test]
    fn no_ridge_in_noise() {
        let num_bins = 513;
        let magnitude = vec![0.1; num_bins]; // flat = noise
        let mut harmonic_field = vec![0.0; num_bins];
        let bin_to_freq = 48000.0 / 1024.0;

        let (f0, _score) = detect_ridges(&magnitude, bin_to_freq, &mut harmonic_field);
        assert!(f0.is_none(), "Should not detect fundamental in noise");
    }

    #[test]
    fn ridge_score_high_for_harmonic() {
        let bin_to_freq = 48000.0 / 1024.0;
        let magnitude = make_harmonic_spectrum(440.0, bin_to_freq, 513);
        let score = ridge_score(&magnitude, 440.0, bin_to_freq);
        assert!(score > 0.3, "Harmonic signal should have high ridge score, got {}", score);
    }
}
