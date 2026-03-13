use crate::types::{Harmonic, HarmonicType};

/// Classify harmonic type based on even/odd ratio.
pub fn classify_type(harmonics: &[Harmonic]) -> HarmonicType {
    let even_energy: f32 = harmonics
        .iter()
        .filter(|h| h.order % 2 == 0)
        .map(|h| h.magnitude * h.magnitude)
        .sum();
    let odd_energy: f32 = harmonics
        .iter()
        .filter(|h| h.order % 2 == 1)
        .map(|h| h.magnitude * h.magnitude)
        .sum();

    if odd_energy < 1e-10 {
        return HarmonicType::Even;
    }

    let ratio = even_energy / odd_energy;

    if ratio > 2.0 {
        HarmonicType::Even
    } else if ratio < 0.5 {
        HarmonicType::Odd
    } else {
        HarmonicType::Both
    }
}
