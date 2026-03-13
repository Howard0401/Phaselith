/// Suppress pre-echo using a tanh fade-in window.
///
/// The first half of the block is attenuated (pre-echo region),
/// the second half is preserved (attack region).
pub fn suppress_pre_echo(block: &mut [f32], strength: f32) {
    let len = block.len();
    if len == 0 {
        return;
    }

    for i in 0..len {
        let t = i as f32 / len as f32;
        // tanh fade-in: suppresses early samples, preserves later ones
        let fade = (t * 4.0 - 2.0).tanh() * 0.5 + 0.5;
        // Blend between original and faded based on strength
        let gain = 1.0 - strength * (1.0 - fade);
        block[i] *= gain;
    }
}
