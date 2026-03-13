/// Expand dynamics to restore micro-dynamics compressed by limiting.
///
/// Uses a downward expander: signals below threshold are attenuated,
/// restoring the dynamic range that was compressed.
pub fn expand_dynamics(samples: &mut [f32], compression_amount: f32, envelope_buf: &mut [f32]) {
    if compression_amount < 0.01 {
        return;
    }

    compute_rms_envelope(samples, envelope_buf, 512);

    let threshold = 0.5; // -6dB
    let ratio = 1.0 + compression_amount; // 1.01 - 2.0

    for (sample, env) in samples.iter_mut().zip(envelope_buf.iter()) {
        if *env < threshold && *env > 1e-10 {
            let gain_db = (*env / threshold).log10() * 20.0 * (1.0 - 1.0 / ratio);
            *sample *= db_to_linear(gain_db);
        }
    }
}

fn compute_rms_envelope(input: &[f32], output: &mut [f32], window_size: usize) {
    let half_win = window_size / 2;
    let len = input.len().min(output.len());
    let mut sum_sq = 0.0f32;

    // Initialize window
    let init_end = window_size.min(len);
    for i in 0..init_end {
        sum_sq += input[i] * input[i];
    }

    let actual_window = init_end.max(1) as f32;

    for i in 0..len {
        let add_idx = (i + half_win).min(len - 1);
        let rem_idx = i.saturating_sub(half_win);
        sum_sq += input[add_idx] * input[add_idx];
        sum_sq -= input[rem_idx] * input[rem_idx];
        sum_sq = sum_sq.max(0.0);

        output[i] = (sum_sq / actual_window).sqrt();
    }
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
