// Format negotiation for APO.
// Validates that the audio format is one we can process.
// APO supports: 32-bit float PCM, mono/stereo only, 44.1/48/96/192 kHz.
//
// Channel support is deliberately limited to 1-2 channels.
// Dual-engine architecture: two independent mono CirrusEngines (L/R) with
// symmetric one-frame-delayed cross-channel context. Each engine maintains
// its own state (frame_index, damage, lattice, fields, validated).
// >2 channels would require additional engines and cross-channel topology.

/// Check if we support the given audio format parameters
pub fn is_format_supported(
    sample_rate: u32,
    bits_per_sample: u16,
    channels: u16,
    is_float: bool,
) -> bool {
    // We require 32-bit float
    if !is_float || bits_per_sample != 32 {
        return false;
    }

    // Supported sample rates
    let supported_rates = [44100, 48000, 96000, 192000];
    if !supported_rates.contains(&sample_rate) {
        return false;
    }

    // Mono or stereo only — engine is stateful, sequential-mono with reset
    // between channels. >2 channels would get incorrect processing.
    if channels < 1 || channels > 2 {
        return false;
    }

    true
}

/// Suggest closest supported format if the requested one isn't supported
pub fn suggest_format(
    sample_rate: u32,
    _bits_per_sample: u16,
    channels: u16,
) -> (u32, u16, u16) {
    // Always suggest 32-bit float
    let bits = 32u16;

    // Snap to nearest supported sample rate
    let rate = match sample_rate {
        0..=45000 => 44100,
        45001..=72000 => 48000,
        72001..=144000 => 96000,
        _ => 192000,
    };

    // Clamp channels to mono/stereo
    let ch = channels.clamp(1, 2);

    (rate, bits, ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_mono_stereo_float32() {
        assert!(is_format_supported(48000, 32, 1, true));
        assert!(is_format_supported(48000, 32, 2, true));
    }

    #[test]
    fn rejects_multichannel() {
        // 5.1, 7.1 etc. must be rejected
        assert!(!is_format_supported(48000, 32, 3, true));
        assert!(!is_format_supported(48000, 32, 6, true));
        assert!(!is_format_supported(48000, 32, 8, true));
    }

    #[test]
    fn rejects_zero_channels() {
        assert!(!is_format_supported(48000, 32, 0, true));
    }

    #[test]
    fn rejects_non_float() {
        assert!(!is_format_supported(48000, 32, 2, false));
        assert!(!is_format_supported(48000, 16, 2, true));
        assert!(!is_format_supported(48000, 24, 2, true));
    }

    #[test]
    fn supports_all_sample_rates() {
        for sr in &[44100u32, 48000, 96000, 192000] {
            assert!(
                is_format_supported(*sr, 32, 2, true),
                "Should support {sr} Hz"
            );
        }
    }

    #[test]
    fn rejects_unsupported_sample_rates() {
        assert!(!is_format_supported(22050, 32, 2, true));
        assert!(!is_format_supported(32000, 32, 2, true));
        assert!(!is_format_supported(88200, 32, 2, true));
    }

    #[test]
    fn suggest_format_clamps_channels_to_stereo() {
        let (_, _, ch) = suggest_format(48000, 32, 6);
        assert_eq!(ch, 2, "6 channels should be clamped to 2");

        let (_, _, ch) = suggest_format(48000, 32, 8);
        assert_eq!(ch, 2, "8 channels should be clamped to 2");
    }

    #[test]
    fn suggest_format_preserves_mono_stereo() {
        let (_, _, ch) = suggest_format(48000, 32, 1);
        assert_eq!(ch, 1);
        let (_, _, ch) = suggest_format(48000, 32, 2);
        assert_eq!(ch, 2);
    }

    #[test]
    fn suggest_format_snaps_sample_rate() {
        let (rate, _, _) = suggest_format(22050, 32, 2);
        assert_eq!(rate, 44100);

        let (rate, _, _) = suggest_format(48000, 32, 2);
        assert_eq!(rate, 48000);

        let (rate, _, _) = suggest_format(88200, 32, 2);
        assert_eq!(rate, 96000);

        let (rate, _, _) = suggest_format(384000, 32, 2);
        assert_eq!(rate, 192000);
    }

    #[test]
    fn suggest_format_always_32bit() {
        let (_, bits, _) = suggest_format(48000, 16, 2);
        assert_eq!(bits, 32);
    }
}
