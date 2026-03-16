# Optimization Survey and Patent-Free Algorithms

## Overview

Comprehensive analysis of all DSP modules (M0–M6) identifying current limitations,
improvement opportunities, and patent-free algorithms available for commercial use.

## Module-by-Module Analysis

### M0: Orchestrator

**Current state:** Routes frames through M1–M6 pipeline, manages FrameClock and hop scheduling.

**No algorithmic issues identified.** M0 is structural glue, not signal processing.

---

### M1: Fingerprint (Damage Detection)

**Current implementation:**
- Cutoff cliff detection with fixed `10.0` energy ratio threshold
- Clipping detection at fixed `0.99` amplitude
- Spectral slope and band flatness computed but **unused** in downstream decisions
- No pre-echo detection
- No resampling artifact detection
- No noise floor estimation

**Limitations:**

| Issue | Impact | Severity |
|-------|--------|----------|
| Fixed cutoff threshold (10.0) | Misses gradual rolloffs (e.g. MP3 @ 192kbps) | High |
| No spectral slope usage | Computed but wasted — could inform M4 extension shape | Medium |
| No band flatness usage | Computed but wasted — could distinguish tonal vs noise content | Medium |
| No pre-echo detection | Can't identify codec pre-echo artifacts for targeted repair | Medium |
| No noise floor estimation | M4/M5 have no noise reference for Wiener-style filtering | Medium |
| No resampling detection | Can't detect 44.1↔48kHz conversion artifacts | Low |

**Improvement opportunities:**
1. **Adaptive cutoff detection** — use spectral slope change-point instead of fixed ratio
2. **Noise floor estimation** — minimum statistics (Martin, 2001) or MMSE noise PSD
3. **Feed spectral slope to M4** — shape the harmonic extension decay curve
4. **Pre-echo fingerprinting** — detect codec pre-ringing for M3 transient repair

---

### M2: Lattice (STFT Analysis)

**Current implementation:**
- StftEngine with Hann window, overlap-add
- TriLattice (core + sub + super) for multi-resolution
- Phase computation via `atan2`

**No major algorithmic issues.** M2 is a well-implemented STFT frontend.

Minor note: sub/super lattices are allocated but usage varies by quality mode.

---

### M3: Factorizer (Harmonic Analysis)

**Current implementation:**
- Top-10 peak buffer for harmonic detection
- Ridge scoring with `1/√n` weights up to 16 harmonics
- Frame-level transient detection (binary `is_transient` flag)
- Spectral flux for transient strength

**Limitations:**

| Issue | Impact | Severity |
|-------|--------|----------|
| Top-10 peak limit | Misses harmonics in dense spectra (e.g. brass, strings) | High |
| No noise floor in peak detection | Quiet harmonics near noise floor get missed | Medium |
| Frame-level transient only | No sub-frame transient localization | Medium |
| No HNM decomposition | Can't separate harmonic vs noise vs transient properly | Medium |
| Ridge scoring weights fixed | `1/√n` doesn't account for instrument-specific patterns | Low |

**Improvement opportunities:**
1. **HNM (Harmonic + Noise Model) decomposition** — separate harmonic, noise, and transient layers (Stylianou, 1996, academic/public domain)
2. **Adaptive peak count** — scale with spectral density instead of fixed 10
3. **Sub-frame transient localization** — use energy envelope analysis within the STFT frame
4. **Noise floor-aware peak picking** — only accept peaks above estimated noise floor from M1

---

### M4: Solver (Harmonic Extension)

**Current implementation:**
- Power-law decay model: `ref_level × (f/f_cutoff)^decay_factor`
- 2-point energy estimation for reference level (bins at cutoff-5, cutoff-10)
- `fast_log2` / `fast_exp2` approximations (~2% error)
- Harmonic field used as binary gate (>0.5 → harmonic, else 0.3 weight)
- Non-harmonic content gets uniform 0.3 weight regardless of content type

**Limitations:**

| Issue | Impact | Severity |
|-------|--------|----------|
| 2-point energy estimation | Unstable reference level, sensitive to spectral dips | High |
| Binary harmonic gate | Wastes M3's continuous harmonic_field values | High |
| Uniform 0.3 non-harmonic weight | Noise and transient content treated identically | Medium |
| fast_log2 ~2% error, no bounds check | Edge cases (near-zero input) can produce NaN | Medium |
| Power-law decay only | Doesn't model spectral envelope shape (formants, resonances) | Medium |

**Improvement opportunities:**
1. **Cepstral spectral envelope** — use real cepstrum or LPC to model the true spectral envelope shape, then extend that shape above cutoff instead of simple power-law decay. This captures formants and resonances that power-law misses. (All public domain: cepstral analysis — Bogert 1963, LPC — Atal 1967, True Envelope — Röbel 2005)
2. **Improved fast_log2/fast_exp2** — add bounds checking, consider 3rd-order polynomial for <0.5% error
3. **Continuous harmonic weighting** — use M3's harmonic_field directly as a continuous weight [0.0, 1.0] instead of binary thresholding
4. **Content-aware non-harmonic weights** — use M1's band flatness to distinguish noise (high flatness → lower weight) from transient (low flatness + high flux → preserve)

---

### M5: Reprojection Validator

**Current implementation:**
- Iterative Wiener soft masking with degradation simulation
- Global MSE convergence check with early exit if error increases
- **NEW: Perceptual adaptive convergence** (psychoacoustic threshold check)
- Degrader: simplified 3-tap MA + tanh clip
- Time-domain residual bypasses validation entirely
- Spectral floor fixed at [0.02, 0.05] range

**Limitations:**

| Issue | Impact | Severity |
|-------|--------|----------|
| Simplified degrader (3-tap MA) | Doesn't model real codec degradation accurately | Medium |
| Time-domain residual unvalidated | Declip output goes directly to M6 without reprojection | Medium |
| Fixed spectral floor [0.02–0.05] | Not adapted to signal characteristics | Low |
| ~~Fixed iteration count~~ | ~~No adaptive stopping~~ | ~~High~~ **FIXED** |

**Improvement opportunities:**
1. **Better degrader model** — use actual codec simulation or learned degradation function
2. **Validate time-domain residual** — apply reprojection or at least energy-based sanity check to declip output
3. **Adaptive spectral floor** — scale floor with M1's noise floor estimate
4. ✅ **Adaptive convergence** — IMPLEMENTED in `psychoacoustic.rs`

---

### M6: Mixer (Perceptual Safety)

**Current implementation:**
- K-weighted loudness (BS.1770-4) — correct 2-stage biquad
- True peak guard — uniform block gain reduction
- Soft crossover sigmoid
- Delayed transient repair with HF de-emphasis
- Ambience preserve with 3-gate design + HP source narrowing
- **NEW: Masking constraint uses shared psychoacoustic module** (Schroeder spreading)

**Limitations:**

| Issue | Impact | Severity |
|-------|--------|----------|
| No temporal masking | Only simultaneous masking, no forward/backward time masking | Medium |
| No tonal/noise distinction | Tonal maskers should have narrower masking curves than noise | Medium |
| Warmth is frequency-blind | `x - w·x³/3` saturates all frequencies equally | Low-Medium |
| Smoothness too simple | 3-tap `[0.25, 0.5, 0.25]` is effectively a ~12kHz LP filter | Low |
| air_brightness placeholder | Style axis defined but M6 doesn't use it | Low (feature gap) |
| spatial_spread placeholder | Style axis defined but M6 doesn't use it | Low (feature gap) |
| body placeholder | Style axis defined but M6 doesn't use it | Low (feature gap) |
| ~~6-point discrete Bark masking~~ | ~~Too coarse~~ | ~~High~~ **FIXED** |

**Improvement opportunities:**
1. **Temporal masking** — add forward masking (~200ms decay) and backward masking (~20ms). Use MPEG-1 psychoacoustic model temporal spreading (patent expired 2017)
2. **Tonal/noise distinction** — use spectral flatness or tonality index (SFM) to classify maskers. Tonal maskers: 14.5 + bark dB offset. Noise maskers: 5.5 dB offset. (MPEG-1 Model 2, all patents expired)
3. **Frequency-aware warmth** — apply more saturation to low-mid frequencies (100-500 Hz) where tube warmth is perceptually strongest, less above 4kHz
4. **Implement style axis effects** — air_brightness (HF shelf boost), spatial_spread (stereo widening via mid/side), body (low-mid emphasis)

---

## Patent-Free Algorithms for Commercial Use

All algorithms below have been verified as free for commercial use as of 2026.

### 1. MPEG-1 Psychoacoustic Model (ISO 11172-3)

**Status:** All patents expired by 2017 (Fraunhofer/Thomson patents).

**Applicable to:** M6 masking improvements

**Key components:**
- Simultaneous masking with spreading function
- Temporal masking (forward + backward)
- Tonal vs noise masker classification
- Signal-to-mask ratio (SMR) computation

### 2. Ephraim-Malah MMSE-STSA (1984)

**Status:** Never patented (published as academic research).

**Applicable to:** M5 improved Wiener filtering

**Key idea:** Minimum mean-square error short-time spectral amplitude estimator. Produces smoother spectral gains than simple Wiener filter, reducing "musical noise" artifacts.

### 3. Cepstral / LPC / True Envelope

**Status:** All public domain (Bogert 1963 / Atal 1967 / Röbel 2005).

**Applicable to:** M4 spectral envelope extraction

**Key idea:** Extract the spectral envelope shape (formants, resonances) using cepstral analysis or linear prediction, then extend that shape above the cutoff frequency. Much more accurate than power-law decay for voice and acoustic instruments.

### 4. Griffin-Lim Algorithm (1984)

**Status:** Never patented (academic publication).

**Applicable to:** M5 phase reconstruction alternative

**Key idea:** Iterative STFT magnitude projection for phase estimation. Could supplement M5's reprojection for phase-sensitive reconstruction.

### 5. Bark / ERB Frequency Scales

**Status:** Public domain (Zwicker 1961 / Moore & Glasberg 1983).

**Applicable to:** All modules using frequency-domain processing

**Already in use** in `psychoacoustic.rs` (Bark scale).

### 6. ITU-R BS.1770 K-Weighting

**Status:** Explicitly patent-free (ITU standard).

**Applicable to:** M6 loudness measurement

**Already in use** in `m6_mixer/kweighting.rs`.

### 7. Sub-Harmonic Synthesis (MaxxBass-style)

**Status:** Core patents expired 2017-2019. **Caution:** Avoid stereo ILD manipulation per US11102577 (still active).

**Applicable to:** M4 or M6 bass enhancement

**Key idea:** Generate sub-harmonics from existing bass content to extend perceived low-frequency response on small speakers.

### 8. Harmonic + Noise Model (HNM)

**Status:** Academic (Stylianou 1996, Serra & Smith 1990). Never patented.

**Applicable to:** M3 harmonic analysis

**Key idea:** Decompose signal into deterministic (harmonic) and stochastic (noise/breath/transient) layers. Enables targeted processing of each layer independently.

### 9. Spectral Subtraction (Boll, 1979)

**Status:** Public domain (45+ years old, academic).

**Applicable to:** M5 noise reduction alternative

**Key idea:** Subtract estimated noise spectrum from noisy signal. Simple but effective baseline for denoising. Musical noise artifacts can be mitigated with oversubtraction factor.

### 10. Equal Loudness Contours (ISO 226:2003)

**Status:** Based on Fletcher-Munson (1933) / Robinson-Dadson (1956). ISO standard, no patent.

**Applicable to:** M6 frequency-dependent level compensation

**Key idea:** Model perceived loudness as a function of frequency and SPL. Could replace the flat warmth saturation with a perceptually-weighted version.

---

## Priority Implementation Order

Based on impact and effort, recommended order:

| Priority | Module | Improvement | Effort | Impact |
|----------|--------|-------------|--------|--------|
| 1 | M4 | Cepstral spectral envelope | Medium | High — fixes most audible artifact (unnatural HF extension) |
| 2 | M4 | Improved fast_log2/fast_exp2 precision | Low | Medium — prevents edge-case NaN |
| 3 | M6 | Temporal masking (forward + backward) | Medium | Medium — reduces over-reconstruction in transient regions |
| 4 | M6 | Tonal/noise masker distinction | Medium | Medium — more accurate masking thresholds |
| 5 | M3 | Adaptive peak count + noise floor | Low | Medium — catches more harmonics in dense spectra |
| 6 | M1 | Adaptive cutoff detection | Low | Medium — handles gradual rolloffs better |
| 7 | M4 | Continuous harmonic weighting | Low | Low-Medium — better use of M3's output |
| 8 | M5 | Better degrader model | High | Medium — more accurate reprojection |
| 9 | M6 | Implement style axis effects | Medium | Low (feature addition) |
| 10 | M3 | HNM decomposition | High | Medium — enables layered processing |

---

## Already Implemented (This Sprint)

| Change | File | Commit |
|--------|------|--------|
| Shared psychoacoustic module | `psychoacoustic.rs` | `de7eae3` |
| M5 adaptive convergence | `m5_reprojection/mod.rs` | `de7eae3` |
| M6 masking refactor (Schroeder spreading) | `m6_mixer/masking.rs` | `de7eae3` |

## Related

- [03-CORE-ALGORITHM.md](03-CORE-ALGORITHM.md) — Module contract overview
- [18-PSYCHOACOUSTIC-MODULE-AND-ADAPTIVE-CONVERGENCE.md](18-PSYCHOACOUSTIC-MODULE-AND-ADAPTIVE-CONVERGENCE.md) — Implementation details for M5/M6 changes
- [17-BUILD-PROFILES-AND-REALTIME-SAFETY.md](17-BUILD-PROFILES-AND-REALTIME-SAFETY.md) — Performance budget context
