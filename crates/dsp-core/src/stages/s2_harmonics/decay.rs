use crate::types::Harmonic;

/// Fit decay curve: magnitude ≈ A × n^(-decay_rate)
/// Using least squares: log(mag) = log(A) - decay_rate × log(n)
///
/// Returns the decay rate (positive number).
pub fn fit_decay_curve(harmonics: &[Harmonic]) -> f32 {
    if harmonics.len() < 2 {
        return 1.0; // Default
    }

    let n_f = harmonics.len() as f32;
    let sum_x: f32 = harmonics.iter().map(|h| (h.order as f32).ln()).sum();
    let sum_y: f32 = harmonics
        .iter()
        .map(|h| h.magnitude.max(1e-10).ln())
        .sum();
    let sum_xy: f32 = harmonics
        .iter()
        .map(|h| (h.order as f32).ln() * h.magnitude.max(1e-10).ln())
        .sum();
    let sum_xx: f32 = harmonics
        .iter()
        .map(|h| (h.order as f32).ln().powi(2))
        .sum();

    let denom = n_f * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return 1.0;
    }

    let slope = (n_f * sum_xy - sum_x * sum_y) / denom;
    (-slope).max(0.0) // Return positive decay rate
}
