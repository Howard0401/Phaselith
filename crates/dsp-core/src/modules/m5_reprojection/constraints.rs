/// Apply hard constraints to the acceptance mask:
/// - Low-band lock: no modification below cutoff
/// - Phase constraint: limit phase correction magnitude
/// - Stereo constraint: bound side/mid ratio
pub fn apply_constraints(mask: &[f32], cutoff_bin: usize) -> Vec<f32> {
    apply_constraints_styled(mask, cutoff_bin, 48000, 1024, 0.0, &[], false, 0.0, 0.0, &[])
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
///
/// `bass_flex` relaxes the impact path without changing the 0% baseline:
/// - widens the impact lane slightly downward/upward
/// - blends in neighboring transient support so bass hits feel less rigid
/// - opens a partial harmonic "flex band" in the low-mids for sustain/bounce
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn neighborhood_max(field: &[f32], center: usize, radius: usize) -> f32 {
    if field.is_empty() {
        return 0.0;
    }
    let start = center.saturating_sub(radius);
    let end = center.saturating_add(radius + 1).min(field.len());
    field[start..end].iter().copied().fold(0.0, f32::max)
}

fn harmonic_gate(harmonic_field: &[f32], bin: usize) -> f32 {
    harmonic_field
        .get(bin)
        .copied()
        .unwrap_or(0.0)
        .max(0.0)
        .sqrt()
        .clamp(0.0, 1.0)
}

pub fn apply_constraints_styled(
    mask: &[f32],
    cutoff_bin: usize,
    sample_rate: u32,
    fft_size: usize,
    impact_gain: f32,
    transient_field: &[f32],
    body_pass_enabled: bool,
    body: f32,
    bass_flex: f32,
    harmonic_field: &[f32],
) -> Vec<f32> {
    let mut constrained = mask.to_vec();
    let bin_to_freq = sample_rate as f32 / fft_size.max(1) as f32;
    let bass_flex = bass_flex.clamp(0.0, 1.0);

    // Impact band: 80-180 Hz at 0%, widening slightly with Bass Flex.
    let impact_lo_bin = (lerp(80.0, 65.0, bass_flex) / bin_to_freq) as usize;
    let impact_hi_bin = (lerp(180.0, 240.0, bass_flex) / bin_to_freq) as usize;
    // Maximum acceptance in impact band (0.0-0.4 range)
    let impact_max = impact_gain.clamp(0.0, 1.0) * lerp(0.4, 0.46, bass_flex);
    // Flex band: low-mid sustain lane that adds a little bounce behind the hit.
    // Keep it controlled inside the 100-500 Hz region, but let Bass Flex
    // reopen a bit more of the low-mid movement.
    let flex_lo_bin = (lerp(180.0, 160.0, bass_flex) / bin_to_freq) as usize;
    let flex_hi_bin = (lerp(180.0, 360.0, bass_flex) / bin_to_freq) as usize;
    let flex_max = 0.11 * bass_flex;
    let body_lo_bin = (180.0 / bin_to_freq) as usize;
    let body_hi_bin = (500.0 / bin_to_freq) as usize;
    let body_active = body_pass_enabled && body > 0.01;
    let flex_active = bass_flex > 0.01;
    let body_pass_max = lerp(0.52, 0.80, body.clamp(0.0, 1.0));

    // Low-band lock: zero below cutoff (with impact band exception)
    for k in 0..cutoff_bin.min(constrained.len()) {
        if body_active
            && k >= body_lo_bin
            && k < body_hi_bin
            && k < harmonic_field.len()
            && harmonic_field[k] > 1e-10
        {
            // Re-open harmonic body bins, but keep a ceiling so the 180-500 Hz
            // lane stays somewhat constrained instead of going fully free.
            let harmonic_support = harmonic_gate(harmonic_field, k);
            let body_gate = body_pass_max * lerp(0.68, 1.0, harmonic_support);
            constrained[k] = (mask[k] * body_gate).min(body_gate);
        } else if impact_gain > 0.01
            && k >= impact_lo_bin
            && k < impact_hi_bin
        {
            // Bass Flex blends direct transient, nearby transient support,
            // and harmonic sustain so kick/bass tails do not feel as rigid.
            let transient_gate = transient_field.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let transient_support = neighborhood_max(transient_field, k, 2).clamp(0.0, 1.0);
            let harmonic_support = harmonic_gate(harmonic_field, k);
            let combined_gate = transient_gate
                .max(transient_support * (0.35 * bass_flex))
                .max(harmonic_support * (0.45 * bass_flex))
                .clamp(0.0, 1.0);
            constrained[k] = (mask[k] * impact_max * combined_gate).min(impact_max);
        } else if flex_active
            && k >= flex_lo_bin
            && k < flex_hi_bin
            && harmonic_gate(harmonic_field, k) > 1e-4
        {
            let transient_support = neighborhood_max(transient_field, k, 2).clamp(0.0, 1.0);
            let harmonic_support = harmonic_gate(harmonic_field, k);
            let flex_gate = (harmonic_support * 0.54 + transient_support * 0.18).clamp(0.0, 1.0);
            constrained[k] = (mask[k] * flex_max * flex_gate).min(flex_max);
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
    bass_flex: f32,
    harmonic_field: &[f32],
    out: &mut [f32],
) {
    let len = mask.len().min(out.len());
    out[..len].copy_from_slice(&mask[..len]);
    let bin_to_freq = sample_rate as f32 / fft_size.max(1) as f32;
    let bass_flex = bass_flex.clamp(0.0, 1.0);

    let impact_lo_bin = (lerp(80.0, 65.0, bass_flex) / bin_to_freq) as usize;
    let impact_hi_bin = (lerp(180.0, 240.0, bass_flex) / bin_to_freq) as usize;
    let impact_max = impact_gain.clamp(0.0, 1.0) * lerp(0.4, 0.46, bass_flex);
    let flex_lo_bin = (lerp(180.0, 160.0, bass_flex) / bin_to_freq) as usize;
    let flex_hi_bin = (lerp(180.0, 360.0, bass_flex) / bin_to_freq) as usize;
    let flex_max = 0.11 * bass_flex;
    let body_lo_bin = (180.0 / bin_to_freq) as usize;
    let body_hi_bin = (500.0 / bin_to_freq) as usize;
    let body_active = body_pass_enabled && body > 0.01;
    let flex_active = bass_flex > 0.01;
    let body_pass_max = lerp(0.52, 0.80, body.clamp(0.0, 1.0));

    for k in 0..cutoff_bin.min(len) {
        if body_active
            && k >= body_lo_bin
            && k < body_hi_bin
            && k < harmonic_field.len()
            && harmonic_field[k] > 1e-10
        {
            let harmonic_support = harmonic_gate(harmonic_field, k);
            let body_gate = body_pass_max * lerp(0.68, 1.0, harmonic_support);
            out[k] = (mask[k] * body_gate).min(body_gate);
        } else if impact_gain > 0.01
            && k >= impact_lo_bin
            && k < impact_hi_bin
        {
            let transient_gate = transient_field.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let transient_support = neighborhood_max(transient_field, k, 2).clamp(0.0, 1.0);
            let harmonic_support = harmonic_gate(harmonic_field, k);
            let combined_gate = transient_gate
                .max(transient_support * (0.35 * bass_flex))
                .max(harmonic_support * (0.45 * bass_flex))
                .clamp(0.0, 1.0);
            out[k] = (mask[k] * impact_max * combined_gate).min(impact_max);
        } else if flex_active
            && k >= flex_lo_bin
            && k < flex_hi_bin
            && harmonic_gate(harmonic_field, k) > 1e-4
        {
            let transient_support = neighborhood_max(transient_field, k, 2).clamp(0.0, 1.0);
            let harmonic_support = harmonic_gate(harmonic_field, k);
            let flex_gate = (harmonic_support * 0.54 + transient_support * 0.18).clamp(0.0, 1.0);
            out[k] = (mask[k] * flex_max * flex_gate).min(flex_max);
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
            0.0,
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
            0.0,
            &harmonic_field,
        );

        let body_energy: f32 = result[lo..hi].iter().sum();
        assert_eq!(body_energy, 0.0, "Body band should stay locked when toggle is off");
    }

    #[test]
    fn bass_flex_widens_impact_band_above_180hz() {
        let mask = vec![1.0; 513];
        let cutoff_bin = 400;
        let sample_rate = 48000;
        let fft_size = 1024;
        let bin_to_freq = sample_rate as f32 / fft_size as f32;
        let flex_bin = (220.0 / bin_to_freq) as usize;
        let mut transient_field = vec![0.0; 513];
        transient_field[flex_bin] = 0.9;

        let base = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.5,
            &transient_field,
            false,
            0.0,
            0.0,
            &[],
        );
        let flexed = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.5,
            &transient_field,
            false,
            0.0,
            1.0,
            &[],
        );

        assert_eq!(base[flex_bin], 0.0, "220 Hz should stay outside the legacy impact lane");
        assert!(flexed[flex_bin] > 0.0, "Bass Flex should widen the impact lane above 180 Hz");
    }

    #[test]
    fn bass_flex_opens_low_mid_harmonic_lane_without_body_pass() {
        let mask = vec![1.0; 513];
        let cutoff_bin = 400;
        let sample_rate = 48000;
        let fft_size = 1024;
        let bin_to_freq = sample_rate as f32 / fft_size as f32;
        let flex_bin = (260.0 / bin_to_freq) as usize;
        let transient_field = vec![0.0; 513];
        let mut harmonic_field = vec![0.0; 513];
        harmonic_field[flex_bin] = 0.36;

        let base = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.0,
            &transient_field,
            false,
            0.4,
            0.0,
            &harmonic_field,
        );
        let flexed = apply_constraints_styled(
            &mask, cutoff_bin, sample_rate, fft_size,
            0.0,
            &transient_field,
            false,
            0.4,
            1.0,
            &harmonic_field,
        );

        assert_eq!(base[flex_bin], 0.0, "Without Bass Flex the low-mid flex lane stays closed");
        assert!(flexed[flex_bin] > 0.0, "Bass Flex should open a partial harmonic sustain lane");
    }
}
