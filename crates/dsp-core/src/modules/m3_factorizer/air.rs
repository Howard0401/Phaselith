/// Extract the air field: stochastic high-frequency content above cutoff.
///
/// Estimates the high-frequency envelope by extrapolating the spectral
/// slope from below the cutoff into the region above it.
pub fn extract_air_field(
    air_magnitude: &[f32],
    bin_to_freq: f32,
    cutoff_bin: usize,
    air_field: &mut [f32],
) {
    air_field.fill(0.0);

    if air_magnitude.is_empty() || cutoff_bin >= air_magnitude.len() {
        return;
    }

    // Compute average magnitude in a band just below cutoff
    let band_width = 20;
    let band_start = cutoff_bin.saturating_sub(band_width);
    let band_end = cutoff_bin.min(air_magnitude.len());

    if band_start >= band_end {
        return;
    }

    let avg_mag: f32 = air_magnitude[band_start..band_end]
        .iter()
        .sum::<f32>()
        / (band_end - band_start) as f32;

    if avg_mag < 1e-10 {
        return;
    }

    // Estimate spectral slope (dB/octave) from the band below cutoff
    let slope = estimate_local_slope(air_magnitude, band_start, band_end, bin_to_freq);

    // Extrapolate above cutoff
    let cutoff_freq = cutoff_bin as f32 * bin_to_freq;
    let len = air_field.len().min(air_magnitude.len());

    for k in cutoff_bin..len {
        let freq = k as f32 * bin_to_freq;
        if freq < cutoff_freq || freq > 22000.0 {
            continue;
        }

        // Extrapolate using slope
        let octaves_above = (freq / cutoff_freq.max(1.0)).log2();
        let db_drop = slope * octaves_above;
        let extrapolated = avg_mag * (10.0f32).powf(db_drop / 20.0);

        air_field[k] = extrapolated.max(0.0);
    }
}

/// Estimate local spectral slope in dB/octave.
fn estimate_local_slope(
    magnitude: &[f32],
    start_bin: usize,
    end_bin: usize,
    bin_to_freq: f32,
) -> f32 {
    if end_bin <= start_bin + 2 {
        return -6.0; // default gentle rolloff
    }

    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_xy = 0.0f64;
    let mut sum_xx = 0.0f64;
    let mut n = 0u32;

    for k in start_bin..end_bin {
        let freq = k as f32 * bin_to_freq;
        if freq < 100.0 || magnitude[k] < 1e-10 {
            continue;
        }

        let x = freq.log2() as f64;
        let y = (20.0 * magnitude[k].log10()) as f64;
        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_xx += x * x;
        n += 1;
    }

    if n < 3 {
        return -6.0;
    }

    let nf = n as f64;
    let denom = nf * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return -6.0;
    }

    let slope = ((nf * sum_xy - sum_x * sum_y) / denom) as f32;
    slope.clamp(-24.0, 0.0) // limit to reasonable range
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_field_extrapolates_above_cutoff() {
        let num_bins = 1025; // air FFT 2048
        let bin_to_freq = 48000.0 / 2048.0; // ~23.4 Hz/bin
        let cutoff_freq = 16000.0;
        let cutoff_bin = (cutoff_freq / bin_to_freq) as usize;

        // Create magnitude that drops to zero above cutoff
        let mut magnitude = vec![0.0; num_bins];
        for k in 0..cutoff_bin {
            let freq = k as f32 * bin_to_freq;
            magnitude[k] = 1.0 / (1.0 + freq / 1000.0); // gentle rolloff
        }

        let mut air_field = vec![0.0; num_bins];
        extract_air_field(&magnitude, bin_to_freq, cutoff_bin, &mut air_field);

        // Should have extrapolated content above cutoff
        let above_cutoff_energy: f32 = air_field[cutoff_bin..].iter().sum();
        assert!(
            above_cutoff_energy > 0.0,
            "Should have energy above cutoff, got {}",
            above_cutoff_energy
        );
    }

    #[test]
    fn air_field_zero_below_cutoff() {
        let num_bins = 513;
        let bin_to_freq = 48000.0 / 1024.0;
        let cutoff_bin = 200;

        let magnitude = vec![1.0; num_bins];
        let mut air_field = vec![0.0; num_bins];
        extract_air_field(&magnitude, bin_to_freq, cutoff_bin, &mut air_field);

        // Below cutoff should be zero
        let below_cutoff: f32 = air_field[..cutoff_bin].iter().sum();
        assert_eq!(below_cutoff, 0.0, "Below cutoff should be zero");
    }

    #[test]
    fn air_field_handles_empty() {
        let mut air_field = vec![0.0; 10];
        extract_air_field(&[], 46.875, 5, &mut air_field);
        assert_eq!(air_field, vec![0.0; 10]);
    }
}
