/// BS.1770-4 K-weighting filter for perceptually accurate loudness measurement.
///
/// Two cascaded biquad stages:
/// 1. Pre-filter (high-shelf): models head-related acoustic coupling
/// 2. RLB weighting (high-pass): de-emphasizes low frequencies
///
/// K-weighted loudness better correlates with perceived loudness than
/// flat RMS — low frequencies contribute less, presence range contributes more.

/// Biquad filter state (Direct Form II Transposed).
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Self { b0, b1, b2, a1, a2, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    fn process_sample(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// K-weighting filter (two cascaded biquads).
#[derive(Debug, Clone)]
pub struct KWeightingFilter {
    stage1: Biquad,
    stage2: Biquad,
}

impl KWeightingFilter {
    /// Create K-weighting filter for the given sample rate.
    /// Uses ITU-R BS.1770-4 coefficients for 48kHz and 44.1kHz.
    /// Other rates fall back to 48kHz coefficients (close enough for level tracking).
    ///
    /// TODO: For 96kHz/192kHz, the 48kHz fallback shifts filter turnover frequencies.
    /// This doesn't break functionality but the measurement is no longer strictly
    /// BS.1770-compliant at those rates. To fix: implement bilinear transform
    /// coefficient generation from the analog prototype, or add pre-computed
    /// coefficients for 96k/192k. Low priority since browser runtime is 48kHz.
    pub fn new(sample_rate: u32) -> Self {
        let (s1, s2) = match sample_rate {
            44100 => (
                // Stage 1: pre-filter (shelving) @ 44100 Hz
                Biquad::new(
                    1.5308412300503478,
                    -2.6509799951547297,
                    1.169079079921587,
                    -1.6636551132560204,
                    0.7125954280732254,
                ),
                // Stage 2: RLB high-pass @ 44100 Hz
                Biquad::new(
                    1.0,
                    -2.0,
                    1.0,
                    -1.9891696736297957,
                    0.9891990357870394,
                ),
            ),
            _ => (
                // Stage 1: pre-filter (shelving) @ 48000 Hz
                Biquad::new(
                    1.53512485958697,
                    -2.69169618940638,
                    1.19839281085285,
                    -1.69065929318241,
                    0.73248077421585,
                ),
                // Stage 2: RLB high-pass @ 48000 Hz
                Biquad::new(
                    1.0,
                    -2.0,
                    1.0,
                    -1.99004745483398,
                    0.99007225036621,
                ),
            ),
        };
        Self { stage1: s1, stage2: s2 }
    }

    /// Compute K-weighted mean square (not RMS — caller takes sqrt if needed).
    /// Processes samples through K-weighting and returns mean(y²).
    ///
    /// IMPORTANT: This advances filter state. For EMA tracking, call once per block.
    pub fn compute_weighted_ms(&mut self, samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let mut sum_sq = 0.0f32;
        for &x in samples {
            let y1 = self.stage1.process_sample(x);
            let y = self.stage2.process_sample(y1);
            sum_sq += y * y;
        }
        sum_sq / samples.len() as f32
    }

    pub fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kweight_filter_creates_for_common_rates() {
        let _f48 = KWeightingFilter::new(48000);
        let _f44 = KWeightingFilter::new(44100);
        let _f96 = KWeightingFilter::new(96000); // falls back to 48k coefficients
    }

    #[test]
    fn kweight_silence_returns_zero() {
        let mut kw = KWeightingFilter::new(48000);
        let silence = vec![0.0f32; 256];
        let ms = kw.compute_weighted_ms(&silence);
        assert!(ms < 1e-20, "Silence should give ~0, got {ms}");
    }

    #[test]
    fn kweight_deemphasizes_low_freq() {
        // 50 Hz sine should be attenuated relative to 2kHz sine (same amplitude)
        let mut kw_low = KWeightingFilter::new(48000);
        let mut kw_high = KWeightingFilter::new(48000);

        let n = 4800; // 100ms
        let low_sine: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 50.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();
        let high_sine: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 2000.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();

        let ms_low = kw_low.compute_weighted_ms(&low_sine);
        let ms_high = kw_high.compute_weighted_ms(&high_sine);

        assert!(
            ms_high > ms_low * 2.0,
            "2kHz should be louder than 50Hz after K-weighting: low={ms_low:.6}, high={ms_high:.6}"
        );
    }

    #[test]
    fn kweight_boosts_presence_range() {
        // 3kHz should get a slight boost vs 1kHz (head model shelf)
        let mut kw_1k = KWeightingFilter::new(48000);
        let mut kw_3k = KWeightingFilter::new(48000);

        let n = 4800;
        let sine_1k: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();
        let sine_3k: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 3000.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();

        let ms_1k = kw_1k.compute_weighted_ms(&sine_1k);
        let ms_3k = kw_3k.compute_weighted_ms(&sine_3k);

        assert!(
            ms_3k > ms_1k,
            "3kHz should be boosted vs 1kHz: 1k={ms_1k:.6}, 3k={ms_3k:.6}"
        );
    }

    #[test]
    fn kweight_reset_clears_state() {
        let mut kw = KWeightingFilter::new(48000);
        let signal: Vec<f32> = (0..480)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        kw.compute_weighted_ms(&signal);
        kw.reset();
        // After reset, processing silence should give near-zero
        let silence = vec![0.0f32; 256];
        let ms = kw.compute_weighted_ms(&silence);
        assert!(ms < 1e-10, "After reset + silence, should be ~0, got {ms}");
    }
}
