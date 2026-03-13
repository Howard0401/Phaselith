use rustfft::{FftPlanner, num_complex::Complex};

/// Detect the high-frequency cutoff of lossy-compressed audio.
/// Returns `None` if no cutoff is detected (lossless).
///
/// Adapted from S0 cutoff detection with energy cliff algorithm.
pub fn detect_cutoff(samples: &[f32], sample_rate: u32, fft_size: usize) -> Option<f32> {
    let len = samples.len().min(fft_size);
    if len < 64 {
        return None;
    }

    let mut buffer: Vec<Complex<f32>> = samples[..len]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let window =
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (len - 1) as f32).cos());
            Complex::new(s * window, 0.0)
        })
        .collect();
    buffer.resize(fft_size, Complex::new(0.0, 0.0));

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut buffer);

    let magnitudes: Vec<f32> = buffer[..fft_size / 2].iter().map(|c| c.norm()).collect();
    let bin_to_freq = sample_rate as f32 / fft_size as f32;

    let band_size = 8.max(fft_size / 256);
    let max_bin = ((20000.0 / bin_to_freq) as usize).min(magnitudes.len() - 1);
    let cliff_ratio = 10.0;

    for center in (band_size..=max_bin.saturating_sub(band_size)).rev() {
        let freq = center as f32 * bin_to_freq;
        if freq > 20000.0 || freq < 2000.0 {
            continue;
        }

        let below_start = center.saturating_sub(band_size);
        let above_end = (center + band_size).min(magnitudes.len() - 1);

        let energy_below: f32 = magnitudes[below_start..center]
            .iter()
            .map(|m| m * m)
            .sum::<f32>()
            / band_size as f32;

        let energy_above: f32 = magnitudes[center..=above_end]
            .iter()
            .map(|m| m * m)
            .sum::<f32>()
            / (above_end - center + 1) as f32;

        if energy_above < f32::EPSILON {
            if energy_below > f32::EPSILON {
                return if freq > 19500.0 { None } else { Some(freq) };
            }
            continue;
        }

        let ratio = energy_below / energy_above;
        if ratio > cliff_ratio {
            return if freq > 19500.0 { None } else { Some(freq) };
        }
    }

    None
}

/// Detect clipping severity (0.0-1.0).
/// Also computes run-length statistics for clipping regions.
pub fn detect_clipping(samples: &[f32]) -> f32 {
    let threshold = 0.99;
    let mut clipped_count = 0u32;
    let mut consecutive = 0u32;
    let mut max_consecutive = 0u32;

    for &sample in samples {
        if sample.abs() >= threshold {
            consecutive += 1;
            clipped_count += 1;
            max_consecutive = max_consecutive.max(consecutive);
        } else {
            consecutive = 0;
        }
    }

    if samples.is_empty() {
        return 0.0;
    }

    let clip_ratio = clipped_count as f32 / samples.len() as f32;
    (clip_ratio * 10.0)
        .min(1.0)
        .max((max_consecutive as f32 / 32.0).min(1.0))
}

/// Estimate dynamic compression/limiting amount from crest factor.
/// Returns 0.0 (healthy) to 1.0 (severely compressed).
pub fn estimate_compression(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();

    if rms < 1e-10 {
        return 0.0;
    }

    let crest_db = 20.0 * (peak / rms).log10();
    ((12.0 - crest_db) / 9.0).clamp(0.0, 1.0)
}

/// Analyze stereo degradation from interleaved samples [L, R, L, R, ...].
/// Returns 0.0 (normal stereo) to 1.0 (severe degradation / mono).
pub fn analyze_stereo_interleaved(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }

    let mut mid_energy = 0.0f32;
    let mut side_energy = 0.0f32;

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
    }

    if mid_energy < 1e-10 {
        return 0.0;
    }

    let side_ratio = side_energy / mid_energy;

    if side_ratio < 0.05 {
        1.0
    } else if side_ratio < 0.1 {
        0.7
    } else if side_ratio < 0.2 {
        0.3
    } else {
        0.0
    }
}

/// Compute spectral slope (dB/octave).
/// Negative slope = natural; very negative = muffled; positive = bright.
pub fn spectral_slope(magnitudes: &[f32], bin_to_freq: f32) -> f32 {
    if magnitudes.len() < 10 {
        return 0.0;
    }

    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_xy = 0.0f64;
    let mut sum_xx = 0.0f64;
    let mut count = 0u32;

    for (i, &mag) in magnitudes.iter().enumerate().skip(1) {
        let freq = i as f32 * bin_to_freq;
        if freq < 100.0 || freq > 16000.0 || mag < 1e-10 {
            continue;
        }
        let x = freq.log2() as f64;
        let y = (20.0 * mag.log10()) as f64;
        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_xx += x * x;
        count += 1;
    }

    if count < 5 {
        return 0.0;
    }

    let n = count as f64;
    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return 0.0;
    }

    ((n * sum_xy - sum_x * sum_y) / denom) as f32
}

/// Compute band flatness (spectral flatness in a frequency range).
/// 1.0 = perfectly flat (noise-like), 0.0 = peaked (tonal).
pub fn band_flatness(magnitudes: &[f32], start_bin: usize, end_bin: usize) -> f32 {
    let end = end_bin.min(magnitudes.len());
    if start_bin >= end || end - start_bin < 2 {
        return 0.0;
    }

    let slice = &magnitudes[start_bin..end];
    let n = slice.len() as f32;

    let geometric_mean = {
        let log_sum: f32 = slice
            .iter()
            .map(|&m| (m.max(1e-10)).ln())
            .sum();
        (log_sum / n).exp()
    };

    let arithmetic_mean = slice.iter().sum::<f32>() / n;

    if arithmetic_mean < 1e-10 {
        return 0.0;
    }

    (geometric_mean / arithmetic_mean).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_clipping_quiet_signal() {
        let samples: Vec<f32> = (0..1024).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        assert!(detect_clipping(&samples) < 0.01);
    }

    #[test]
    fn detect_clipping_loud_signal() {
        let mut samples: Vec<f32> = (0..1024)
            .map(|i| 2.0 * (i as f32 * 0.1).sin())
            .collect();
        for s in &mut samples {
            if s.abs() >= 0.98 {
                *s = s.signum() * 1.0;
            }
        }
        assert!(detect_clipping(&samples) > 0.1);
    }

    #[test]
    fn compression_silence_is_zero() {
        let samples = vec![0.0f32; 1024];
        assert_eq!(estimate_compression(&samples), 0.0);
    }

    #[test]
    fn compression_returns_valid_range() {
        let samples: Vec<f32> = (0..4096)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let result = estimate_compression(&samples);
        assert!(result >= 0.0 && result <= 1.0);
    }

    #[test]
    fn stereo_mono_shows_high_degradation() {
        let mut interleaved = Vec::new();
        for i in 0..512 {
            let v = (i as f32 * 0.1).sin();
            interleaved.push(v);
            interleaved.push(v); // same L and R = mono
        }
        let result = analyze_stereo_interleaved(&interleaved);
        assert!(result > 0.5, "Mono signal should show high degradation, got {}", result);
    }

    #[test]
    fn stereo_different_channels_low_degradation() {
        let mut interleaved = Vec::new();
        for i in 0..512 {
            interleaved.push((i as f32 * 0.1).sin());
            interleaved.push((i as f32 * 0.13).sin());
        }
        let result = analyze_stereo_interleaved(&interleaved);
        assert!(result < 0.5, "Different L/R should show low degradation, got {}", result);
    }

    #[test]
    fn spectral_slope_of_flat_spectrum() {
        let magnitudes = vec![1.0; 512];
        let slope = spectral_slope(&magnitudes, 48000.0 / 1024.0);
        assert!(slope.abs() < 5.0, "Flat spectrum should have near-zero slope, got {}", slope);
    }

    #[test]
    fn band_flatness_of_noise() {
        // All equal magnitudes = perfectly flat
        let magnitudes = vec![1.0; 100];
        let flatness = band_flatness(&magnitudes, 10, 90);
        assert!(flatness > 0.9, "Flat spectrum should have high flatness, got {}", flatness);
    }

    #[test]
    fn band_flatness_of_tone() {
        // Single peak = very peaked
        let mut magnitudes = vec![0.001; 100];
        magnitudes[50] = 10.0;
        let flatness = band_flatness(&magnitudes, 10, 90);
        assert!(flatness < 0.5, "Tonal spectrum should have low flatness, got {}", flatness);
    }
}
