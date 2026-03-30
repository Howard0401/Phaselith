# 20 — Cepstral Spectral Envelope, HighFidelity Mode, and Extreme Dynamics Protection

## Overview

This document covers three related changes introduced together:

1. **M4 Cepstral Spectral Envelope** — replaces power-law decay for harmonic extension
2. **HighFidelity QualityMode** — decouples FFT size from reprojection iterations
3. **Extreme Dynamics Protection** — three safety layers against clipping artifacts

---

## A. HighFidelity QualityMode

### Problem

APO used `UltraExtreme` (FFT 8192, hop 2048) which tied large FFT to high reprojection iterations. The 8192 FFT created 42.7 ms frames — too long for transient preservation. Meanwhile, Chrome extension used `Standard` (FFT 1024, hop 256) with only 2 reprojection iterations but had far better transient handling.

APO also set `pre_echo_transient_scale = 0.4` to compensate for the 8192 FFT smearing sibilants, but this suppressed legitimate transient detail.

### Solution

New `QualityMode::HighFidelity`:

| Mode | FFT | Hop | Iters | Time Resolution |
|------|-----|-----|-------|-----------------|
| Standard | 1024 | 256 | 2 | 5.3 ms |
| **HighFidelity** | **1024** | **256** | **5** | **5.3 ms** |
| Extreme | 2048 | 512 | 5 | 10.7 ms |
| UltraExtreme | 8192 | 2048 | 8 | 42.7 ms |

HighFidelity keeps the 1024 FFT (good temporal resolution) while allowing 5 reprojection iterations (more validation depth). APO now uses HighFidelity with `pre_echo_transient_scale = 1.0`.

### Files Changed

- `crates/dsp-core/src/config.rs` — new variant
- `crates/apo-dll/src/apo_impl.rs` — quality_mode + transient scale
- `crates/dsp-core/src/modules/m7_governor.rs` — match arms

---

## B. Extreme Dynamics Protection

Three safety layers to prevent distortion during large dynamic swings (forte→piano, drum hits, clipped sources):

### B1: OLA Accumulation Clamp

**File**: `crates/dsp-core/src/modules/m5_reprojection/overlap_add.rs`

```rust
self.accum[idx] = (self.accum[idx] + frame[i]).clamp(-2.0, 2.0);
```

Prevents coherent peak accumulation from exceeding ±2.0 during overlap-add.

### B2: Time-Domain Residual Energy Gate

**File**: `crates/dsp-core/src/modules/m5_reprojection/mod.rs`

Declip residuals bypass M5 frequency-domain validation. Added energy-proportional gate:

```
if |residual[k]| > 2 × |dry[k]|:
    residual[k] *= (2 × |dry[k]|) / |residual[k]|
```

Ensures time-domain corrections never exceed 2× the original signal energy at any sample.

### B3: Makeup Gain Strict Clamp

**File**: `crates/dsp-core/src/modules/m6_mixer/mod.rs`

```rust
samples[i] = (s * headroom_gain).clamp(-limit, limit);
```

Previously, makeup gain could push warmth-clamped values (0.95) back above 1.0. Now clamp is applied after gain.

---

## C. M4 Cepstral Spectral Envelope

### Motivation

The original power-law decay model estimates a single decay rate from the overall spectral tilt and applies a smooth `ref × 2^(decay × octaves)` rolloff above cutoff. This is simple and sounds "warm" but cannot capture formant structure, resonances, or instrument-specific spectral shapes.

### Algorithm (Bogert 1963, public domain)

```
magnitude → ln(mag) → IFFT → real cepstrum
                                    ↓
                              lifter (keep L=25 low quefrency)
                                    ↓
                              FFT → smooth log-envelope → exp()
                                    ↓
                              extrapolate above cutoff (slope regression)
```

**Lifter cutoff L=25**: At 48 kHz / 1024 FFT, quefrency index 25 corresponds to ~1920 Hz formant spacing. This preserves vocal and acoustic instrument formant structure while removing individual harmonic detail.

### Key Design Decisions

- Pre-allocated buffers in `init_with_plans()` — zero allocations during `compute()`
- Shared FFT plans with M2 and M5 via `SharedFftPlans`
- 2-bin raised-cosine taper at lifter boundaries to prevent Gibbs ringing
- Fallback to power-law when `cutoff_bin < 15` or NaN/Inf
- Crossfade between cepstral and power-law for `cutoff_bin` in [15, 75)

### Perceptual Comparison: Power-Law vs Cepstral

#### Power-law characteristics ("渾圓" / rounded warmth)

1. **Uniform energy fill**: Every bin above cutoff receives smoothly decaying energy regardless of spectral shape. Creates a warm "halo" that fills all frequencies evenly.
2. **Conservative decay rate**: `estimate_decay_rate()` returns -18 to 0 dB/octave from full below-cutoff region. The broad averaging produces gentler slopes.
3. **Shape-agnostic**: Does not track formant valleys or resonance peaks. Spectral valleys above cutoff get filled just as much as peaks → perceptually "rounded."
4. **Higher reference level**: Uses `average_magnitude_near(cutoff, 5)` — 10-bin average can be higher than the actual envelope at cutoff.

#### Cepstral characteristics (natural but thinner)

1. **Faithful envelope tracking**: Follows the actual spectral envelope shape including formant valleys → more transparent, less colored.
2. **Conservative extrapolation clamp**: `target.min(envelope[cutoff-1])` prevents extrapolated energy from exceeding the cutoff level. This is acoustically correct but perceptually too strict.
3. **Steeper slope tendency**: Linear regression on 20 bins near cutoff can capture local steep rolloff that the power-law's global estimate would smooth over.
4. **Net result**: Less total HF energy above cutoff → sounds more accurate but loses the "body" that power-law provided.

#### Resolution

Two adjustments to preserve cepstral accuracy while restoring warmth:

1. **Relax extrapolation clamp**: Allow extrapolated envelope up to `1.5 × envelope[cutoff-1]` instead of strict `≤ cutoff level`. This restores some of the energy headroom that power-law naturally provided.
2. **Slope floor**: Clamp regression slope to be no steeper than the power-law estimate. This prevents local steep features near cutoff from producing overly aggressive rolloff.

### Performance

| Item | Cost |
|------|------|
| Cepstral compute() | ~15–19 μs/frame |
| Per-hop budget (48 kHz, hop=256) | 5333 μs |
| Overhead | ~0.3% |

### Files Changed

- `crates/dsp-core/src/modules/m4_solver/harmonic_ext.rs` — CepstralEnvelope struct + algorithm
- `crates/dsp-core/src/modules/m4_solver/mod.rs` — cepstral_env field + init_with_plans()
- `crates/dsp-core/src/engine.rs` — M4 wiring in build_default()

---

## Constraints

- **Zero allocation**: All `process()` paths are allocation-free
- **Patent-free**: Cepstral analysis is Bogert 1963 (public domain), liftering is standard DSP textbook technique
- **M4 changes not yet committed**: User requested M4 cepstral changes remain uncommitted pending listening validation
