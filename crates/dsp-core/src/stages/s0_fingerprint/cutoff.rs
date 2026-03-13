use rustfft::{FftPlanner, num_complex::Complex};

/// Detect the high-frequency cutoff of lossy-compressed audio.
///
/// Lossy codecs (AAC/MP3/OGG) truncate all content above a certain frequency.
/// In the STFT spectrum, this appears as a sharp "cliff" where energy drops
/// to the noise floor.
///
/// Returns `None` if no cutoff is detected (lossless).
pub fn detect_cutoff(samples: &[f32], sample_rate: u32, fft_size: usize) -> Option<f32> {
    let len = samples.len().min(fft_size);
    if len < 64 {
        return None;
    }

    // Build FFT input with Hann window
    let mut buffer: Vec<Complex<f32>> = samples[..len]
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (len - 1) as f32).cos());
            Complex::new(s * window, 0.0)
        })
        .collect();

    // Pad to fft_size if needed
    buffer.resize(fft_size, Complex::new(0.0, 0.0));

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);
    fft.process(&mut buffer);

    let magnitudes: Vec<f32> = buffer[..fft_size / 2]
        .iter()
        .map(|c| c.norm())
        .collect();

    let bin_to_freq = sample_rate as f32 / fft_size as f32;

    // Use energy cliff detection: find where average energy drops dramatically
    // compared to the band just below it. This is more robust than absolute thresholds.
    let band_size = 8.max(fft_size / 256); // ~8-16 bins per band
    let max_bin = ((20000.0 / bin_to_freq) as usize).min(magnitudes.len() - 1);

    // Scan from high to low, compute average energy in sliding bands
    // Find where energy ratio between adjacent bands exceeds a cliff threshold
    let cliff_ratio = 10.0; // Band below must have 10x more energy than band above

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

    None // No cliff detected = lossless
}

#[allow(dead_code)]
fn estimate_noise_floor(magnitudes: &[f32]) -> f32 {
    let mut sorted: Vec<f32> = magnitudes.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let percentile_10 = sorted.len() / 10;
    if percentile_10 < sorted.len() {
        sorted[percentile_10]
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_lowpassed_sine(sample_rate: u32, size: usize, max_freq: f32) -> Vec<f32> {
        // Generate a broadband signal with content only below max_freq
        // Use many closely-spaced tones to fill the spectrum densely
        let mut samples = vec![0.0f32; size];
        let bin_width = sample_rate as f32 / size as f32;
        let num_tones = (max_freq / bin_width) as usize;
        for i in 1..=num_tones {
            let freq = i as f32 * bin_width;
            if freq >= max_freq {
                break;
            }
            let amplitude = 1.0 / (1.0 + (i as f32).sqrt());
            for (j, sample) in samples.iter_mut().enumerate() {
                *sample += amplitude
                    * (2.0 * std::f32::consts::PI * freq * j as f32 / sample_rate as f32).sin();
            }
        }
        // Normalize
        let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak > 0.0 {
            for s in &mut samples {
                *s /= peak;
            }
        }
        samples
    }

    #[test]
    fn detects_no_cutoff_for_wideband_signal() {
        let samples = generate_lowpassed_sine(48000, 2048, 20000.0);
        let result = detect_cutoff(&samples, 48000, 2048);
        // Should be None (lossless) or > 19500
        assert!(
            result.is_none() || result.unwrap() > 19000.0,
            "Expected no cutoff for wideband signal, got {:?}",
            result
        );
    }

    #[test]
    fn detects_cutoff_for_lowpassed_signal() {
        let samples = generate_lowpassed_sine(48000, 2048, 8000.0);
        let result = detect_cutoff(&samples, 48000, 2048);
        assert!(result.is_some(), "Expected cutoff for 8kHz lowpassed signal");
        let cutoff = result.unwrap();
        assert!(
            cutoff < 12000.0,
            "Expected cutoff below 12kHz, got {}",
            cutoff
        );
    }
}
