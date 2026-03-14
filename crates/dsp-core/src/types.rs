// ─── CIRRUS Core Data Structures ───

/// Maximum FFT size supported (air lattice).
pub const MAX_FFT_SIZE: usize = 4096;
/// Micro lattice FFT size.
pub const MICRO_FFT_SIZE: usize = 256;
/// Core lattice FFT size.
pub const CORE_FFT_SIZE: usize = 1024;
/// Air lattice FFT size.
pub const AIR_FFT_SIZE: usize = 2048;

// ─── M1: Damage Posterior ───

/// Gaussian estimate N(μ, σ²) for a single damage dimension.
#[derive(Debug, Clone, Copy)]
pub struct GaussianEstimate {
    pub mean: f32,
    pub variance: f32,
}

impl Default for GaussianEstimate {
    fn default() -> Self {
        Self {
            mean: 0.0,
            variance: 1.0, // high initial uncertainty
        }
    }
}

impl GaussianEstimate {
    pub fn new(mean: f32, variance: f32) -> Self {
        Self { mean, variance }
    }

    /// Standard deviation.
    pub fn std_dev(&self) -> f32 {
        self.variance.sqrt()
    }

    /// Confidence = 1 / (1 + σ). High variance → low confidence.
    pub fn confidence(&self) -> f32 {
        1.0 / (1.0 + self.std_dev())
    }
}

/// Factorized posterior over all damage dimensions.
/// P(θ|x) ≈ P(cutoff|x) · P(clip|x) · P(limit|x) · ...
#[derive(Debug, Clone)]
pub struct DamagePosterior {
    /// Cutoff frequency estimate (Hz). mean=20000 means lossless.
    pub cutoff: GaussianEstimate,
    /// Clipping severity (0-1).
    pub clipping: GaussianEstimate,
    /// Limiting/compression amount (0-1).
    pub limiting: GaussianEstimate,
    /// Pre-echo severity (0-1).
    pub pre_echo: GaussianEstimate,
    /// Stereo collapse severity (0-1).
    pub stereo_collapse: GaussianEstimate,
    /// Resampler smear amount (0-1).
    pub resampler_smear: GaussianEstimate,
    /// Noise floor lift amount (dB).
    pub noise_lift: GaussianEstimate,
    /// Per-band confidence map (num_bands entries).
    pub confidence_map: Vec<f32>,
    /// Overall detection confidence (0-1).
    pub overall_confidence: f32,
}

impl Default for DamagePosterior {
    fn default() -> Self {
        Self {
            cutoff: GaussianEstimate::new(20000.0, 1000.0),
            clipping: GaussianEstimate::default(),
            limiting: GaussianEstimate::default(),
            pre_echo: GaussianEstimate::default(),
            stereo_collapse: GaussianEstimate::default(),
            resampler_smear: GaussianEstimate::default(),
            noise_lift: GaussianEstimate::default(),
            confidence_map: Vec::new(),
            overall_confidence: 0.0,
        }
    }
}

impl DamagePosterior {
    /// Check if damage is significant enough to warrant processing.
    pub fn needs_processing(&self) -> bool {
        self.cutoff.mean < 19500.0
            || self.clipping.mean > 0.05
            || self.limiting.mean > 0.1
            || self.pre_echo.mean > 0.05
            || self.stereo_collapse.mean > 0.1
    }

    /// Reset to no-damage state.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

// ─── M2: Tri-Lattice ───

/// A single STFT lattice (one window size).
#[derive(Debug, Clone)]
pub struct Lattice {
    /// FFT size used for this lattice.
    pub fft_size: usize,
    /// Magnitude spectrum |X(m,k)|.
    pub magnitude: Vec<f32>,
    /// Phase spectrum ∠X(m,k).
    pub phase: Vec<f32>,
    /// Energy field P_l(m,k) = |X|².
    pub energy: Vec<f32>,
    /// Group delay G_l(m,k) in samples.
    pub group_delay: Vec<f32>,
}

impl Default for Lattice {
    fn default() -> Self {
        Self {
            fft_size: 0,
            magnitude: Vec::new(),
            phase: Vec::new(),
            energy: Vec::new(),
            group_delay: Vec::new(),
        }
    }
}

impl Lattice {
    pub fn new(fft_size: usize) -> Self {
        let num_bins = fft_size / 2 + 1;
        Self {
            fft_size,
            magnitude: vec![0.0; num_bins],
            phase: vec![0.0; num_bins],
            energy: vec![0.0; num_bins],
            group_delay: vec![0.0; num_bins],
        }
    }

    pub fn num_bins(&self) -> usize {
        self.magnitude.len()
    }

    pub fn clear(&mut self) {
        self.magnitude.fill(0.0);
        self.phase.fill(0.0);
        self.energy.fill(0.0);
        self.group_delay.fill(0.0);
    }
}

/// Tri-lattice: three concurrent STFT analyses at different resolutions.
#[derive(Debug, Clone)]
pub struct TriLattice {
    /// Short window (256 samples): transient detail.
    pub micro: Lattice,
    /// Medium window (1024 samples): main synthesis.
    pub core: Lattice,
    /// Long window (2048 samples): high-freq stability.
    pub air: Lattice,
}

impl Default for TriLattice {
    fn default() -> Self {
        Self {
            micro: Lattice::default(),
            core: Lattice::default(),
            air: Lattice::default(),
        }
    }
}

impl TriLattice {
    pub fn new() -> Self {
        Self {
            micro: Lattice::new(MICRO_FFT_SIZE),
            core: Lattice::new(CORE_FFT_SIZE),
            air: Lattice::new(AIR_FFT_SIZE),
        }
    }

    pub fn clear(&mut self) {
        self.micro.clear();
        self.core.clear();
        self.air.clear();
    }
}

// ─── M3: Structured Fields ───

/// Decomposed signal fields from M3 Structured Factorizer.
#[derive(Debug, Clone)]
pub struct StructuredFields {
    /// Harmonic ridge field H(m,k): energy belonging to harmonic series.
    pub harmonic: Vec<f32>,
    /// Transient field T(m,k): onset/attack energy.
    pub transient: Vec<f32>,
    /// Air/noise field A(m,k): stochastic high-frequency content.
    pub air: Vec<f32>,
    /// Spatial field Q(m,k): mid/side consistency score.
    pub spatial: Vec<f32>,
    /// Detected fundamental frequency (Hz), if any.
    pub fundamental_freq: Option<f32>,
    /// Ridge score R(f0, m) for the detected fundamental.
    pub ridge_score: f32,
    /// Spectral flux transient flag: true when significant spectral change detected.
    /// Computed from frame-to-frame magnitude difference (L² norm).
    /// Downstream modules can use this for adaptive behavior (e.g. relax OLA, hold gains).
    pub is_transient: bool,
    /// Raw spectral flux value (normalized, 0-1 range).
    pub spectral_flux: f32,
}

impl Default for StructuredFields {
    fn default() -> Self {
        Self {
            harmonic: Vec::new(),
            transient: Vec::new(),
            air: Vec::new(),
            spatial: Vec::new(),
            fundamental_freq: None,
            ridge_score: 0.0,
            is_transient: false,
            spectral_flux: 0.0,
        }
    }
}

impl StructuredFields {
    pub fn new(num_bins: usize) -> Self {
        Self {
            harmonic: vec![0.0; num_bins],
            transient: vec![0.0; num_bins],
            air: vec![0.0; num_bins],
            spatial: vec![0.0; num_bins],
            fundamental_freq: None,
            ridge_score: 0.0,
            is_transient: false,
            spectral_flux: 0.0,
        }
    }

    pub fn clear(&mut self) {
        self.harmonic.fill(0.0);
        self.transient.fill(0.0);
        self.air.fill(0.0);
        self.spatial.fill(0.0);
        self.fundamental_freq = None;
        self.ridge_score = 0.0;
        self.is_transient = false;
        self.spectral_flux = 0.0;
    }
}

// ─── M4: Residual Candidate ───

/// The "missing residual" computed by M4 Inverse Residual Solver.
/// Only contains what needs to be *added* to restore the signal.
///
/// All fields are **freq-domain** (per-bin). Time-domain residuals
/// (e.g. declip corrections) live in `ProcessContext::time_candidate`.
#[derive(Debug, Clone)]
pub struct ResidualCandidate {
    /// Freq-domain: harmonic continuation residual (magnitude per-bin, above cutoff).
    pub harmonic: Vec<f32>,
    /// Freq-domain: air-field continuation residual (magnitude per-bin).
    pub air: Vec<f32>,
    /// Freq-domain: phase correction residual (per-bin).
    pub phase: Vec<f32>,
    /// Spatial control: side/mid recovery coefficients (ratio per-bin, 0-ρ_max).
    /// Not freq-domain magnitude — needs cross-channel context to apply.
    pub side: Vec<f32>,
}

impl Default for ResidualCandidate {
    fn default() -> Self {
        Self {
            harmonic: Vec::new(),
            air: Vec::new(),
            phase: Vec::new(),
            side: Vec::new(),
        }
    }
}

impl ResidualCandidate {
    pub fn new(num_bins: usize) -> Self {
        Self {
            harmonic: vec![0.0; num_bins],
            air: vec![0.0; num_bins],
            phase: vec![0.0; num_bins],
            side: vec![0.0; num_bins],
        }
    }

    pub fn clear(&mut self) {
        self.harmonic.fill(0.0);
        self.air.fill(0.0);
        self.phase.fill(0.0);
        self.side.fill(0.0);
    }
}

// ─── M5: Validated Residual ───

/// Output of M5 Self-Reprojection Validator.
/// Contains only the residual that passed consistency checks.
#[derive(Debug, Clone)]
pub struct ValidatedResidual {
    /// Time-domain validated residual to be added to dry signal (from freq-domain path).
    pub data: Vec<f32>,
    /// Time-domain residual from declip/transient repair (bypasses freq-domain validation).
    /// Uses independent gain path in M6 — not scaled by freq-domain consistency.
    pub time_residual: Vec<f32>,
    /// Per-sample acceptance mask (0.0 = rejected, 1.0 = fully accepted).
    pub acceptance_mask: Vec<f32>,
    /// Overall consistency score C_cons (0-1). Higher = more consistent.
    pub consistency_score: f32,
    /// Reprojection error J_rep.
    pub reprojection_error: f32,
}

impl Default for ValidatedResidual {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            time_residual: Vec::new(),
            acceptance_mask: Vec::new(),
            consistency_score: 0.0,
            reprojection_error: f32::MAX,
        }
    }
}

impl ValidatedResidual {
    pub fn new(num_samples: usize) -> Self {
        Self {
            data: vec![0.0; num_samples],
            time_residual: vec![0.0; num_samples],
            acceptance_mask: vec![0.0; num_samples],
            consistency_score: 0.0,
            reprojection_error: f32::MAX,
        }
    }

    pub fn clear(&mut self) {
        self.data.fill(0.0);
        self.time_residual.fill(0.0);
        self.acceptance_mask.fill(0.0);
        self.consistency_score = 0.0;
        self.reprojection_error = f32::MAX;
    }
}

// ─── Cross-Channel Analysis Context ───

/// Stereo analysis context computed from dry L/R input.
/// Used as gate/bias for side recovery — adjusts aggressiveness
/// without replacing per-bin spatial_field analysis.
///
/// Computed once per frame from dry (pre-processing) L and R input.
/// Both channels receive the same context (from previous frame)
/// to ensure symmetric processing.
#[derive(Debug, Clone, Copy)]
pub struct CrossChannelContext {
    /// Pearson correlation between L and R (-1 to 1).
    /// High (near 1) = mono-like → more aggressive side recovery.
    /// Low = true stereo content → conservative recovery.
    pub correlation: f32,
    /// Energy of mid signal: (L+R)/2
    pub mid_energy: f32,
    /// Energy of side signal: (L-R)/2
    pub side_energy: f32,
    /// Stereo width: side_energy / (mid_energy + ε)
    pub stereo_width: f32,
}

impl Default for CrossChannelContext {
    fn default() -> Self {
        Self {
            correlation: 0.0,
            mid_energy: 0.0,
            side_energy: 0.0,
            stereo_width: 0.0,
        }
    }
}

impl CrossChannelContext {
    /// Compute cross-channel context from L and R dry input buffers.
    pub fn from_lr(left: &[f32], right: &[f32]) -> Self {
        let len = left.len().min(right.len());
        if len == 0 {
            return Self::default();
        }

        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;
        let mut sum_ll = 0.0f32;
        let mut sum_rr = 0.0f32;
        let mut sum_lr = 0.0f32;
        let mut mid_e = 0.0f32;
        let mut side_e = 0.0f32;

        for i in 0..len {
            let l = left[i];
            let r = right[i];
            sum_l += l;
            sum_r += r;
            sum_ll += l * l;
            sum_rr += r * r;
            sum_lr += l * r;

            let mid = (l + r) * 0.5;
            let side = (l - r) * 0.5;
            mid_e += mid * mid;
            side_e += side * side;
        }

        let n = len as f32;
        mid_e /= n;
        side_e /= n;

        // Pearson correlation
        let mean_l = sum_l / n;
        let mean_r = sum_r / n;
        let var_l = (sum_ll / n - mean_l * mean_l).max(0.0);
        let var_r = (sum_rr / n - mean_r * mean_r).max(0.0);
        let cov_lr = sum_lr / n - mean_l * mean_r;
        let denom = (var_l * var_r).sqrt();
        let correlation = if denom > 1e-12 {
            (cov_lr / denom).clamp(-1.0, 1.0)
        } else {
            // Degenerate case: near-zero variance in one or both channels.
            // Check if L ≈ R (both near-constant and equal) → mono-like → correlation = 1.0
            // Otherwise (e.g., one channel is silence, other has signal) → 0.0
            let diff_energy: f32 = left.iter().zip(right.iter())
                .take(len)
                .map(|(l, r)| { let d = l - r; d * d })
                .sum::<f32>() / n;
            if diff_energy <= 1e-10 { 1.0 } else { 0.0 }
        };

        let epsilon = 1e-10;
        let stereo_width = side_e / (mid_e + epsilon);

        Self {
            correlation,
            mid_energy: mid_e,
            side_energy: side_e,
            stereo_width,
        }
    }
}

// ─── Legacy types (kept for backward compat during migration) ───

/// Quality tier based on detected cutoff frequency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityTier {
    Low,
    Medium,
    High,
    Lossless,
}

/// Legacy degradation profile (used by old stages, will be removed).
#[derive(Debug, Clone, Copy)]
pub struct DegradationProfile {
    pub cutoff_freq: Option<f32>,
    pub quality_tier: QualityTier,
    pub clipping_severity: f32,
    pub compression_amount: f32,
    pub stereo_degradation: f32,
    pub confidence: f32,
}

impl Default for DegradationProfile {
    fn default() -> Self {
        Self {
            cutoff_freq: None,
            quality_tier: QualityTier::Lossless,
            clipping_severity: 0.0,
            compression_amount: 0.0,
            stereo_degradation: 0.0,
            confidence: 0.0,
        }
    }
}

/// Legacy harmonic map (used by old stages, will be removed).
#[derive(Debug, Clone)]
pub struct HarmonicMap {
    pub tracks: Vec<FundamentalTrack>,
    pub noise_envelope: Vec<f32>,
}

impl Default for HarmonicMap {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            noise_envelope: Vec::new(),
        }
    }
}

impl HarmonicMap {
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.noise_envelope.clear();
    }
}

#[derive(Debug, Clone)]
pub struct FundamentalTrack {
    pub freq: f32,
    pub harmonics: Vec<Harmonic>,
    pub decay_rate: f32,
    pub harmonic_type: HarmonicType,
}

#[derive(Debug, Clone, Copy)]
pub struct Harmonic {
    pub order: usize,
    pub freq: f32,
    pub magnitude: f32,
    pub phase: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarmonicType {
    Even,
    Odd,
    Both,
    Noise,
}

/// Legacy constants.
pub const MAX_HARMONIC_TRACKS: usize = 32;
pub const MAX_HARMONICS_PER_TRACK: usize = 64;
pub const SPECTRUM_DISPLAY_BINS: usize = 512;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_estimate_confidence() {
        let precise = GaussianEstimate::new(15000.0, 0.01);
        let uncertain = GaussianEstimate::new(15000.0, 100.0);
        assert!(precise.confidence() > uncertain.confidence());
    }

    #[test]
    fn gaussian_estimate_default_is_uncertain() {
        let est = GaussianEstimate::default();
        assert_eq!(est.mean, 0.0);
        assert_eq!(est.variance, 1.0);
    }

    #[test]
    fn damage_posterior_needs_processing() {
        let mut dp = DamagePosterior::default();
        assert!(!dp.needs_processing()); // cutoff 20000 = lossless

        dp.cutoff.mean = 15000.0;
        assert!(dp.needs_processing());
    }

    #[test]
    fn damage_posterior_clear_resets() {
        let mut dp = DamagePosterior::default();
        dp.cutoff.mean = 8000.0;
        dp.clipping.mean = 0.5;
        dp.clear();
        assert!(!dp.needs_processing());
    }

    #[test]
    fn lattice_new_has_correct_bins() {
        let l = Lattice::new(1024);
        assert_eq!(l.num_bins(), 513); // 1024/2 + 1
        assert_eq!(l.fft_size, 1024);
    }

    #[test]
    fn tri_lattice_new_sizes() {
        let tl = TriLattice::new();
        assert_eq!(tl.micro.fft_size, MICRO_FFT_SIZE);
        assert_eq!(tl.core.fft_size, CORE_FFT_SIZE);
        assert_eq!(tl.air.fft_size, AIR_FFT_SIZE);
    }

    #[test]
    fn structured_fields_clear() {
        let mut sf = StructuredFields::new(512);
        sf.harmonic[0] = 1.0;
        sf.fundamental_freq = Some(440.0);
        sf.clear();
        assert_eq!(sf.harmonic[0], 0.0);
        assert!(sf.fundamental_freq.is_none());
    }

    #[test]
    fn residual_candidate_clear() {
        let mut rc = ResidualCandidate::new(256);
        rc.harmonic[0] = 1.0;
        rc.clear();
        assert_eq!(rc.harmonic[0], 0.0);
    }

    #[test]
    fn validated_residual_default() {
        let vr = ValidatedResidual::default();
        assert_eq!(vr.consistency_score, 0.0);
        assert_eq!(vr.reprojection_error, f32::MAX);
    }
}
