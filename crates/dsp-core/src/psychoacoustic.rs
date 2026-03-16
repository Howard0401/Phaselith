//! Shared psychoacoustic utilities for the Phaselith DSP pipeline.
//!
//! Provides Terhardt absolute hearing threshold, Bark scale conversions,
//! and a continuous spreading function for simultaneous masking.
//! Used by M5 (adaptive convergence) and M6 (masking constraint).
//!
//! All functions are `#[inline]` and allocation-free for real-time safety.

/// Absolute hearing threshold in quiet (Terhardt, 1979).
///
/// Returns the threshold in **linear amplitude** (not dB).
/// The classic 4-term approximation models the U-shaped sensitivity curve:
/// most sensitive ~3-4 kHz (ear canal resonance), rising sharply at LF and HF.
///
/// Reference: Terhardt, E. (1979). "Calculating virtual pitch."
///            Hearing Research, 1(2), 155–182.
#[inline]
pub fn absolute_threshold_linear(freq_hz: f32) -> f32 {
    let f_khz = freq_hz / 1000.0;
    if f_khz < 0.02 {
        return 1.0; // below audible range
    }
    let threshold_db = 3.64 * f_khz.powf(-0.8)
        - 6.5 * (-0.6 * (f_khz - 3.3) * (f_khz - 3.3)).exp()
        + 1e-3 * f_khz * f_khz * f_khz * f_khz;
    // Offset -96 dB: reference level mapping from SPL to 16-bit float range.
    // At 96 dB dynamic range, silence threshold ≈ 1 LSB of 16-bit audio.
    db_to_linear(threshold_db - 96.0)
}

/// Absolute hearing threshold in **dB** (no reference offset).
/// Useful for spreading function calculations that work in the dB domain.
#[inline]
pub fn absolute_threshold_db(freq_hz: f32) -> f32 {
    let f_khz = freq_hz / 1000.0;
    if f_khz < 0.02 {
        return 96.0;
    }
    3.64 * f_khz.powf(-0.8)
        - 6.5 * (-0.6 * (f_khz - 3.3) * (f_khz - 3.3)).exp()
        + 1e-3 * f_khz * f_khz * f_khz * f_khz
}

/// Convert frequency in Hz to Bark scale (Terhardt, 1979).
///
/// Valid range: ~20 Hz to ~20 kHz → ~0.2 to ~24.0 Bark.
#[inline]
pub fn hz_to_bark(freq: f32) -> f32 {
    13.0 * (0.00076 * freq).atan() + 3.5 * (freq / 7500.0).powi(2).atan()
}

/// Convert Bark scale back to Hz.
#[inline]
pub fn bark_to_hz(bark: f32) -> f32 {
    1960.0 * (bark + 0.53) / (26.28 - bark).max(0.01)
}

/// Simultaneous masking spreading function (Schroeder, 1979).
///
/// Given a masker at `masker_bark` with energy `masker_db` (in dB),
/// returns the masking effect at `target_bark` in dB.
///
/// The spreading function has asymmetric slopes:
/// - Lower slope (masker masks upward): -27 dB/Bark
/// - Upper slope (masker masks downward): varies with level,
///   approximately -(24 + 230/f_center - 0.2·masker_db) dB/Bark
///
/// This models the basilar membrane mechanics: excitation spreads
/// more broadly upward (toward higher frequencies) at high levels.
///
/// Reference: Schroeder, M.R., Atal, B.S., Hall, J.L. (1979).
///            "Optimizing digital speech coders by exploiting masking
///            properties of the human ear." JASA, 66(6), 1647–1652.
#[inline]
pub fn spreading_function_db(masker_bark: f32, target_bark: f32, masker_db: f32) -> f32 {
    let dz = target_bark - masker_bark;
    if dz.abs() < 0.01 {
        return masker_db; // same critical band
    }

    let spread = if dz > 0.0 {
        // Upper slope (masker is below target in frequency)
        // Steeper: -24 dB/Bark base, level-dependent broadening
        let slope = -(24.0 + 230.0 / bark_to_hz(masker_bark).max(100.0) - 0.2 * masker_db.max(0.0));
        masker_db + slope * dz
    } else {
        // Lower slope (masker is above target in frequency)
        // -27 dB/Bark (roughly constant)
        masker_db - 27.0 * dz.abs()
    };

    spread
}

/// Compute the composite masking threshold at a given frequency bin.
///
/// Combines:
/// 1. Absolute hearing threshold (Terhardt)
/// 2. Simultaneous masking from all significant spectral components
///
/// `spectrum_magnitudes` should contain per-bin magnitudes (linear amplitude).
/// Returns the threshold in **linear amplitude**.
///
/// This function scans Bark-spaced neighbors (up to ±5 Bark) for efficiency.
/// In a full MPEG-1 Model 2 this would be a dense convolution; we approximate
/// with discrete Bark sampling which is adequate for convergence decisions.
///
/// `use_simultaneous`: if false, returns only absolute threshold (conservative).
pub fn masking_threshold(
    bin: usize,
    bin_to_freq: f32,
    spectrum_magnitudes: &[f32],
    use_simultaneous: bool,
) -> f32 {
    let freq = bin as f32 * bin_to_freq;
    let absolute = absolute_threshold_linear(freq);

    if !use_simultaneous || spectrum_magnitudes.is_empty() {
        return absolute;
    }

    let target_bark = hz_to_bark(freq);

    // Scan neighbors within ±5 Bark (covers ~1 octave each direction at 1 kHz)
    // Step in 0.5 Bark increments for better resolution than M6's old 6-point approach
    let mut max_masking_linear = 0.0f32;

    // Convert target bin's own energy to dB for reference
    let num_bins = spectrum_magnitudes.len();

    // Iterate over nearby Bark bands
    let mut neighbor_bark = (target_bark - 5.0).max(0.5);
    while neighbor_bark <= target_bark + 5.0 {
        if (neighbor_bark - target_bark).abs() < 0.25 {
            neighbor_bark += 0.5;
            continue; // skip self
        }

        let neighbor_freq = bark_to_hz(neighbor_bark);
        let neighbor_bin = (neighbor_freq / bin_to_freq) as usize;

        if neighbor_bin < num_bins && neighbor_bin != bin {
            let mag = spectrum_magnitudes[neighbor_bin];
            if mag > 1e-10 {
                let masker_db = linear_to_db(mag);
                let spread_db = spreading_function_db(neighbor_bark, target_bark, masker_db);
                if spread_db > -96.0 {
                    let spread_linear = db_to_linear(spread_db);
                    max_masking_linear = max_masking_linear.max(spread_linear);
                }
            }
        }

        neighbor_bark += 0.5;
    }

    // Masking offset: simultaneous masking threshold is typically
    // 5-6 dB below the masker for tonal maskers, ~2 dB for noise.
    // We use a conservative 5.5 dB offset (tonal assumption — safer).
    let masked_threshold = max_masking_linear * 0.53; // -5.5 dB ≈ ×0.53

    absolute.max(masked_threshold)
}

/// Check if per-bin errors are below the perceptual threshold.
///
/// Returns `true` if the reprojection has converged perceptually:
/// at least `pass_ratio` fraction of bins have error below threshold.
///
/// This is M5's adaptive convergence criterion. Uses absolute threshold
/// only (conservative) — stopping when errors are below the hearing
/// threshold in quiet guarantees no audible difference from further
/// iterations.
///
/// # Arguments
/// * `error` - Per-bin reprojection error magnitudes
/// * `cutoff_bin` - First bin above the lossy cutoff (below = locked to dry)
/// * `core_bins` - Total number of frequency bins
/// * `bin_to_freq` - Hz per bin (sample_rate / fft_size)
/// * `pass_ratio` - Fraction of bins that must pass (e.g. 0.95 = 95%)
pub fn is_perceptually_converged(
    error: &[f32],
    cutoff_bin: usize,
    core_bins: usize,
    bin_to_freq: f32,
    pass_ratio: f32,
) -> bool {
    let start = cutoff_bin;
    let end = core_bins.min(error.len());
    if end <= start {
        return true; // no bins to check
    }

    let total = end - start;
    let mut below_threshold = 0usize;

    for k in start..end {
        let freq = k as f32 * bin_to_freq;
        let threshold = absolute_threshold_linear(freq);

        // Use error magnitude directly against threshold.
        // Error is |D(x+r) - x| which is in the same linear amplitude domain.
        if error[k] <= threshold {
            below_threshold += 1;
        }
    }

    below_threshold as f32 / total as f32 >= pass_ratio
}

// ─── Utility conversions ───

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[inline]
pub fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.max(1e-20).log10()
}

// ─── Tests ───

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_u_shaped() {
        let t_500 = absolute_threshold_linear(500.0);
        let t_3000 = absolute_threshold_linear(3000.0);
        let t_15000 = absolute_threshold_linear(15000.0);

        // Most sensitive around 3-4 kHz
        assert!(t_3000 < t_500, "3kHz should be more sensitive than 500Hz");
        assert!(t_3000 < t_15000, "3kHz should be more sensitive than 15kHz");
    }

    #[test]
    fn threshold_db_matches_linear() {
        for &freq in &[500.0, 1000.0, 3000.0, 8000.0, 15000.0] {
            let db = absolute_threshold_db(freq);
            let from_linear = linear_to_db(absolute_threshold_linear(freq)) + 96.0;
            assert!(
                (db - from_linear).abs() < 0.1,
                "DB mismatch at {freq}Hz: direct={db:.2}, from_linear={from_linear:.2}"
            );
        }
    }

    #[test]
    fn bark_hz_roundtrip() {
        // Terhardt Bark formula has limited roundtrip accuracy at high frequencies.
        // 200-4000 Hz: <10% error. Above 4kHz: up to ~20% error is expected.
        for &(freq, tol) in &[(200.0, 0.1), (1000.0, 0.1), (4000.0, 0.15), (10000.0, 0.25)] {
            let bark = hz_to_bark(freq);
            let back = bark_to_hz(bark);
            assert!(
                (back - freq).abs() / freq < tol,
                "Roundtrip error too large at {freq}Hz: got {back:.1}Hz (tol={tol})"
            );
        }
    }

    #[test]
    fn spreading_function_asymmetric() {
        let masker_bark = hz_to_bark(1000.0);
        let masker_db = 60.0;

        // Upper spread (masker below target → target at higher freq)
        let up_1bark = spreading_function_db(masker_bark, masker_bark + 1.0, masker_db);
        let up_2bark = spreading_function_db(masker_bark, masker_bark + 2.0, masker_db);

        // Lower spread (masker above target → target at lower freq)
        let down_1bark = spreading_function_db(masker_bark, masker_bark - 1.0, masker_db);
        let down_2bark = spreading_function_db(masker_bark, masker_bark - 2.0, masker_db);

        // Both should decrease with distance
        assert!(up_1bark > up_2bark, "Upper spread should decrease");
        assert!(down_1bark > down_2bark, "Lower spread should decrease");

        // Lower slope is steeper (27 dB/Bark vs ~24 dB/Bark)
        assert!(
            down_1bark < up_1bark,
            "Lower slope should be steeper: down={down_1bark:.1} vs up={up_1bark:.1}"
        );
    }

    #[test]
    fn convergence_check_passes_for_tiny_errors() {
        let error = vec![1e-8; 100];
        assert!(is_perceptually_converged(&error, 10, 100, 48000.0 / 8192.0, 0.95));
    }

    #[test]
    fn convergence_check_fails_for_large_errors() {
        let error = vec![1.0; 100]; // way above any threshold
        assert!(!is_perceptually_converged(&error, 10, 100, 48000.0 / 8192.0, 0.95));
    }

    #[test]
    fn masking_threshold_absolute_only() {
        // With use_simultaneous=false, should equal absolute threshold
        let bin_to_freq = 48000.0 / 8192.0; // ~5.86 Hz/bin
        let freq = 4000.0;
        let bin = (freq / bin_to_freq) as usize;
        // Recompute exact freq from bin to avoid rounding mismatch
        let exact_freq = bin as f32 * bin_to_freq;
        let t = masking_threshold(bin, bin_to_freq, &[], false);
        let abs_t = absolute_threshold_linear(exact_freq);
        assert!(
            (t - abs_t).abs() < 1e-6,
            "Without simultaneous masking, should equal absolute threshold: t={t:.10}, abs={abs_t:.10}"
        );
    }

    #[test]
    fn masking_threshold_raised_by_loud_neighbor() {
        let bin_to_freq = 48000.0 / 8192.0; // ~5.86 Hz/bin
        let num_bins = 4097;

        // Target: bin 500 ≈ 2930 Hz (Bark ~4.8)
        // Masker: bin 350 ≈ 2051 Hz (Bark ~3.5) → ~1.3 Bark away
        let target_bin = 500;

        // Silent spectrum
        let silent = vec![0.0f32; num_bins];
        let t_silent = masking_threshold(target_bin, bin_to_freq, &silent, true);

        // Loud neighbor ~1.3 Bark away
        let mut loud = vec![1e-4f32; num_bins]; // tiny noise floor
        loud[350] = 0.5; // strong component
        let t_loud = masking_threshold(target_bin, bin_to_freq, &loud, true);

        assert!(
            t_loud > t_silent,
            "Loud neighbor should raise masking threshold: silent={t_silent:.8}, loud={t_loud:.8}"
        );
    }
}
