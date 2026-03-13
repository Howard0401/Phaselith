// Format negotiation for APO.
// Validates that the audio format is one we can process.
// APO supports: 32-bit float PCM, mono/stereo only, 44.1/48/96/192 kHz.
//
// Channel support is deliberately limited to 1-2 channels.
// CirrusEngine is stateful (frame_index, damage, lattice, fields, validated
// all accumulate). The current sequential-mono architecture resets between
// channels, which prevents state contamination but means >2 channels would
// get incorrect processing. Future: stereo-native architecture.

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
