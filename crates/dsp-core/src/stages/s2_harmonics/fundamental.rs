/// A detected spectral peak (fundamental candidate).
pub struct SpectralPeak {
    pub freq: f32,
    pub magnitude: f32,
    #[allow(dead_code)]
    pub bin: usize,
}

/// Find spectral peaks in magnitude spectrum.
/// A peak is a local maximum above the median magnitude.
pub fn find_spectral_peaks(magnitudes: &[f32], bin_to_freq: f32) -> Vec<SpectralPeak> {
    let mut peaks = Vec::new();

    if magnitudes.len() < 5 {
        return peaks;
    }

    // Compute median magnitude as threshold
    let mut sorted = magnitudes.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    let threshold = median * 4.0; // Peak must be significantly above median

    for i in 2..magnitudes.len() - 2 {
        let m = magnitudes[i];
        if m > threshold
            && m > magnitudes[i - 1]
            && m > magnitudes[i + 1]
            && m > magnitudes[i - 2]
            && m > magnitudes[i + 2]
        {
            peaks.push(SpectralPeak {
                freq: i as f32 * bin_to_freq,
                magnitude: m,
                bin: i,
            });
        }
    }

    // Sort by magnitude descending, keep top N
    peaks.sort_by(|a, b| b.magnitude.partial_cmp(&a.magnitude).unwrap_or(std::cmp::Ordering::Equal));
    peaks.truncate(32); // Max 32 fundamentals

    peaks
}
