use crate::frame::SynthesisMode;
use crate::types::Lattice;

/// Synthesize time-domain output from validated frequency-domain residual.
///
/// Dispatches to the appropriate synthesis path based on `SynthesisMode`.
/// Currently only `LegacyAdditive` is implemented; FFT-based modes are stubs
/// that will be filled in Phase B1.
pub fn synthesize(
    mode: SynthesisMode,
    combined: &[f32],
    phase: &[f32],
    cutoff_bin: usize,
    fft_size: usize,
    scale: f32,
    output: &mut [f32],
) {
    match mode {
        SynthesisMode::LegacyAdditive => {
            synthesize_additive(combined, phase, cutoff_bin, fft_size, scale, output);
        }
        SynthesisMode::FftOlaPilot | SynthesisMode::FftOlaFull => {
            // Stub: will be implemented in Phase B1.
            // For now, fall back to LegacyAdditive so sound never breaks.
            synthesize_additive(combined, phase, cutoff_bin, fft_size, scale, output);
        }
    }
}

/// Legacy additive synthesis: freq-domain → time-domain via sum of cosines.
///
///   x[n] = (2/N) Σ_k R[k] · cos(2π·k·n/N + φ[k])
///
/// where R[k] is accepted residual magnitude and φ[k] is lattice phase.
/// Factor 2/N accounts for real-signal symmetry (positive freqs only).
fn synthesize_additive(
    combined: &[f32],
    phase: &[f32],
    cutoff_bin: usize,
    fft_size: usize,
    scale: f32,
    output: &mut [f32],
) {
    let out_len = output.len();
    for i in 0..out_len {
        output[i] = 0.0;
    }

    if fft_size == 0 || scale < 1e-6 {
        return;
    }

    let inv_fft = 2.0 / fft_size as f32;
    let two_pi_over_n = std::f32::consts::TAU / fft_size as f32;
    let num_synth_bins = combined.len().min(phase.len());

    for n in 0..out_len {
        let mut sum = 0.0f32;
        let omega_n = two_pi_over_n * n as f32;

        for k in cutoff_bin..num_synth_bins {
            let mag = combined[k];
            if mag.abs() < 1e-8 {
                continue;
            }
            let ph = phase[k];
            sum += mag * (omega_n * k as f32 + ph).cos();
        }
        output[n] = sum * scale * inv_fft;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additive_zero_residual_produces_zero() {
        let combined = vec![0.0; 513];
        let phase = vec![0.0; 513];
        let mut output = vec![1.0; 128]; // non-zero to verify it gets cleared
        synthesize(
            SynthesisMode::LegacyAdditive,
            &combined, &phase, 0, 1024, 1.0, &mut output,
        );
        assert!(output.iter().all(|&s| s.abs() < 1e-8));
    }

    #[test]
    fn additive_single_bin_produces_cosine() {
        let mut combined = vec![0.0; 513];
        let phase = vec![0.0; 513];
        combined[10] = 0.5;

        let mut output = vec![0.0; 128];
        synthesize(
            SynthesisMode::LegacyAdditive,
            &combined, &phase, 0, 1024, 1.0, &mut output,
        );

        let energy: f32 = output.iter().map(|s| s * s).sum();
        assert!(energy > 0.0, "Single bin should produce output");
        assert!(output.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn pilot_mode_falls_back_to_additive() {
        let mut combined = vec![0.0; 513];
        let phase = vec![0.0; 513];
        combined[10] = 0.5;

        let mut output_legacy = vec![0.0; 128];
        let mut output_pilot = vec![0.0; 128];

        synthesize(SynthesisMode::LegacyAdditive, &combined, &phase, 0, 1024, 1.0, &mut output_legacy);
        synthesize(SynthesisMode::FftOlaPilot, &combined, &phase, 0, 1024, 1.0, &mut output_pilot);

        // Until B1 implements FFT path, pilot should produce identical output
        for i in 0..128 {
            assert!(
                (output_legacy[i] - output_pilot[i]).abs() < 1e-8,
                "Pilot fallback should match legacy at sample {}",
                i
            );
        }
    }

    #[test]
    fn cutoff_skips_low_bins() {
        let mut combined = vec![0.0; 513];
        let phase = vec![0.0; 513];
        // Energy only in bins 0..10, cutoff at bin 100
        for k in 0..10 {
            combined[k] = 0.5;
        }

        let mut output = vec![0.0; 128];
        synthesize(
            SynthesisMode::LegacyAdditive,
            &combined, &phase, 100, 1024, 1.0, &mut output,
        );

        // All energy is below cutoff → zero output
        assert!(output.iter().all(|&s| s.abs() < 1e-8));
    }

    #[test]
    fn zero_scale_produces_zero() {
        let mut combined = vec![0.0; 513];
        let phase = vec![0.0; 513];
        combined[200] = 1.0;

        let mut output = vec![0.0; 128];
        synthesize(
            SynthesisMode::LegacyAdditive,
            &combined, &phase, 0, 1024, 0.0, &mut output,
        );
        assert!(output.iter().all(|&s| s.abs() < 1e-8));
    }
}
