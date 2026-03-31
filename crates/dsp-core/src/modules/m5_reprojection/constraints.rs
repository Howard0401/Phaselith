/// Apply hard constraints to the acceptance mask:
/// - Low-band lock: no modification below cutoff
/// - Phase constraint: limit phase correction magnitude
/// - Stereo constraint: bound side/mid ratio
pub fn apply_constraints(mask: &[f32], cutoff_bin: usize) -> Vec<f32> {
    apply_constraints_styled(mask, cutoff_bin, 48000, 1024, 0.0, &[], false, 0.0, &[])
}

/// Apply constraints with impact band support.
///
/// Opens a narrow window in the 80-180 Hz region for transient-shaped
/// residual, controlled by `impact_gain`. Only allows residual through
/// where transient energy is detected (no sustained bass inflation).
///
/// When `body_pass_enabled` is true, a second exception re-opens
/// harmonic bins in the 180-500 Hz body band so the M4 body path can
/// survive the low-band lock for Windows extension A/B testing.
pub fn apply_constraints_styled(
    mask: &[f32],
    cutoff_bin: usize,
    sample_rate: u32,
    fft_size: usize,
    impact_gain: f32,
    transient_field: &[f32],
    body_pass_enabled: bool,
    body: f32,
    harmonic_field: &[f32],
) -> Vec<f32> {
    let mut constrained = mask.to_vec();
    let bin_to_freq = sample_rate as f32 / fft_size.max(1) as f32;

    // Impact band: 80-180 Hz
    let impact_lo_bin = (80.0 / bin_to_freq) as usize;
    let impact_hi_bin = (180.0 / bin_to_freq) as usize;
    // Maximum acceptance in impact band (0.0-0.4 range)
    let impact_max = impact_gain.clamp(0.0, 1.0) * 0.4;
    let body_lo_bin = (180.0 / bin_to_freq) as usize;
    let body_hi_bin = (500.0 / bin_to_freq) as usize;
    let body_active = body_pass_enabled && body > 0.01;

    // Low-band lock: zero below cutoff (with impact band exception)
    for k in 0..cutoff_bin.min(constrained.len()) {
        if impact_gain > 0.01
            && k >= impact_lo_bin
            && k < impact_hi_bin
            && k < transient_field.len()
        {
            // Allow transient-shaped residual in impact band
            let transient_gate = transient_field[k].clamp(0.0, 1.0);
            // Only open when transient energy is detected
            constrained[k] = (mask[k] * impact_max * transient_gate).min(impact_max);
        } else if body_active
            && k >= body_lo_bin
            && k < body_hi_bin
            && k < harmonic_field.len()
            && harmonic_field[k] > 1e-10
        {
            // Only re-open bins that M4 already tagged as harmonic body content.
            constrained[k] = mask[k];
        } else {
            constrained[k] = 0.0;
        }
    }

    // Soft transition zone around cutoff (5 bins)
    let transition_width = 5;
    for k in cutoff_bin..cutoff_bin.saturating_add(transition_width).min(constrained.len()) {
        let t = (k - cutoff_bin) as f32 / transition_width as f32;
        constrained[k] *= t; // ramp up
    }

    constrained
}

/// Zero-alloc variant: writes constrained mask into pre-allocated `out` buffer.
#[cfg(feature = "native-rt")]
pub fn apply_constraints_styled_into(
    mask: &[f32],
    cutoff_bin: usize,
    sample_rate: u32,
    fft_size: usize,
    impact_gain: f32,
    transient_field: &[f32],
    body_pass_enabled: bool,
    body: f32,
    harmonic_field: &[f32],
    out: &mut [f32],
) {
    let len = mask.len().min(out.len());
    out[..len].copy_from_slice(&mask[..len]);
    let bin_to_freq = sample_rate as f32 / fft_size.max(1) as f32;

    let impact_lo_bin = (80.0 / bin_to_freq) as usize;
    let impact_hi_bin = (180.0 / bin_to_freq) as usize;
    let impact_max = impact_gain.clamp(0.0, 1.0) * 0.4;
    let body_lo_bin = (180.0 / bin_to_freq) as usize;
    let body_hi_bin = (500.0 / bin_to_freq) as usize;
    let body_active = body_pass_enabled && body > 0.01;

    for k in 0..cutoff_bin.min(len) {
        if impact_gain > 0.01
            && k >= impact_lo_bin
            && k < impact_hi_bin
            && k < transient_field.len()
        {
            let transient_gate = transient_field[k].clamp(0.0, 1.0);
            out[k] = (mask[k] * impact_max * transient_gate).min(impact_max);
        } else if body_active
            && k >= body_lo_bin
            && k < body_hi_bin
            && k < harmonic_field.len()
            && harmonic_field[k] > 1e-10
        {
            out[k] = mask[k];
        } else {
            out[k] = 0.0;
        }
    }

    let transition_width = 5;
    for k in cutoff_bin..cutoff_bin.saturating_add(transition_width).min(len) {
        let t = (k - cutoff_bin) as f32 / transition_width as f32;
        out[k] *= t;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_lock_below_cutoff() {
        let mask = vec![1.0; 100];
        let result = apply_constraints(&mask, 50);

        for k in 0..50 {
            assert_eq!(result[k], 0.0, "Should be locked below cutoff");
        }
    }

    #[test]
    fn constraints_transition_zone() {
        let mask = vec![1.0; 100];
        let cutoff = 50;
        let result = apply_constraints(&mask, cutoff);

        // Just above cutoff should be ramped
        assert!(result[cutoff] < result[cutoff + 4]);
        // Well above cutoff should be full
        assert!((result[cutoff + 10] - 1.0).abs() < 0.01);
    }

    #[test]
    fn constraints_preserve_above_transition() {
        let mask = vec![0.8; 100];
        let result = apply_constraints(&mask, 10);

        // Well above cutoff should preserve original mask value
        assert!((result[50] - 0.8).abs() < 0.01);
    }

    #[test]
    fn impact_band_opens_for_transients() {
        let mask = vec![1.0; 513];
        let cutoff_bin = 400; // cutoff well above impact band
        let sample_rate = 48000;
        let fft_size = 1024;
        let bin_to_freq = sample_rate as f32 / fft_size as f32;

        // Create transient field with energy in 80-180 Hz
        let mut transient_field = vec![0.0; 513];
        let lo = (80.0 / bin_to_freq) as usize;
        let hi = (180.0 / bin_to_freq) as usize;
        for k in lo..hi {
            transient_field[k] = 0.8; // strong transient
        }

        let result = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.5, // impact_gain
            &transient_field,
            false,
            0.0,
            &[],
        );

        // Impact band should have non-zero values
        let impact_energy: f32 = result[lo..hi].iter().sum();
        assert!(impact_energy > 0.0, "Impact band should be open for transients");

        // But below impact band should still be zero
        for k in 0..lo {
            assert_eq!(result[k], 0.0, "Below impact band should be locked");
        }
    }

    #[test]
    fn impact_band_stays_closed_without_transients() {
        let mask = vec![1.0; 513];
        let cutoff_bin = 400;
        let transient_field = vec![0.0; 513]; // no transients

        let result = apply_constraints_styled(
            &mask, cutoff_bin, 48000, 1024,
            0.5, // impact_gain
            &transient_field,
            false,
            0.0,
            &[],
        );

        // Impact band should remain zero without transient energy
        let lo = (80.0 / (48000.0 / 1024.0)) as usize;
        let hi = (180.0 / (48000.0 / 1024.0)) as usize;
        let impact_energy: f32 = result[lo..hi].iter().sum();
        assert_eq!(impact_energy, 0.0, "No transients = no impact opening");
    }

    #[test]
    fn body_band_opens_for_harmonic_bins_when_enabled() {
        let mask = vec![1.0; 513];
        let cutoff_bin = 400;
        let sample_rate = 48000;
        let fft_size = 1024;
        let bin_to_freq = sample_rate as f32 / fft_size as f32;
        let lo = (180.0 / bin_to_freq) as usize;
        let hi = (500.0 / bin_to_freq) as usize;
        let mut harmonic_field = vec![0.0; 513];
        for k in lo..hi.min(harmonic_field.len()) {
            if k % 3 == 0 {
                harmonic_field[k] = 0.2;
            }
        }

        let result = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.0,
            &[],
            true,
            0.4,
            &harmonic_field,
        );

        let body_energy: f32 = result[lo..hi].iter().sum();
        assert!(body_energy > 0.0, "Body band should open on harmonic bins when enabled");
    }

    #[test]
    fn body_band_stays_closed_when_toggle_disabled() {
        let mask = vec![1.0; 513];
        let cutoff_bin = 400;
        let sample_rate = 48000;
        let fft_size = 1024;
        let bin_to_freq = sample_rate as f32 / fft_size as f32;
        let lo = (180.0 / bin_to_freq) as usize;
        let hi = (500.0 / bin_to_freq) as usize;
        let mut harmonic_field = vec![0.0; 513];
        for k in lo..hi.min(harmonic_field.len()) {
            harmonic_field[k] = 0.2;
        }

        let result = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.0,
            &[],
            false,
            0.4,
            &harmonic_field,
        );

        let body_energy: f32 = result[lo..hi].iter().sum();
        assert_eq!(body_energy, 0.0, "Body band should stay locked when toggle is off");
    }
}
