/// Detect clipping severity in audio samples.
///
/// Clipping occurs when audio exceeds full scale (|sample| >= threshold).
/// Returns severity 0.0-1.0.
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
    let severity = (clip_ratio * 10.0)
        .min(1.0)
        .max((max_consecutive as f32 / 32.0).min(1.0));

    severity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_clipping_in_quiet_signal() {
        let samples: Vec<f32> = (0..1024).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        assert!(detect_clipping(&samples) < 0.01);
    }

    #[test]
    fn detects_hard_clipping() {
        let mut samples: Vec<f32> = (0..1024)
            .map(|i| 2.0 * (i as f32 * 0.1).sin()) // Amplitude > 1.0
            .collect();
        // Hard clip
        for s in &mut samples {
            *s = s.clamp(-0.99, 0.99);
        }
        // This won't trigger because we clamp AT the threshold
        // Let's make some samples exactly at 0.99
        for s in &mut samples {
            if s.abs() >= 0.98 {
                *s = s.signum() * 1.0; // Force above threshold
            }
        }
        assert!(detect_clipping(&samples) > 0.1);
    }
}
