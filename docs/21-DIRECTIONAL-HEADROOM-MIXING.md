# Directional Headroom Mixing

## Problem

The M6 PerceptualSafetyMixer had a gain-staging bug that caused audible distortion (hard clipping) during loud passages, particularly in the Windows APO runtime.

## Root Cause

Two separate issues were identified in the mixer's residual limiting logic.

### Issue 1: Per-sample hard clamp (design flaw)

The original code mixed dry + residual, then applied a hard clamp as a safety net:

```rust
let mixed = dry + residual;
if mixed.abs() > limit {
    let headroom = (limit - dry.abs()).max(0.0);
    samples[i] = dry + residual.signum() * headroom.min(residual.abs());
} else {
    samples[i] = mixed;
}
samples[i] = samples[i].clamp(-limit, limit);  // ← hard clamp
```

When `dry` itself exceeded the limit (e.g., dry = 0.9999 with limit = 0.95), `headroom = 0`, the residual was zeroed out, and the final `.clamp()` hard-cut the dry signal from 0.9999 to 0.95. Consecutive samples in loud passages all getting clipped to 0.95 produces flat-topped waveforms — classic hard clipping distortion with audible high-frequency harmonics.

This was fixed by removing the per-sample `.clamp()` and relying on `true_peak_guard` for block-level uniform gain reduction, which preserves waveform shape.

### Issue 2: Non-directional headroom formula (formula bug)

The headroom calculation `limit - |dry|` does not account for the sign relationship between dry and residual:

```rust
let headroom = (limit - dry.abs()).max(0.0);
```

This formula is only correct when dry and residual have the **same sign**. When they have **opposite signs**, the available space before hitting the limit in the residual's direction is `limit + |dry|`, not `limit - |dry|`.

**Example of the bug:**
- `dry = 0.30`, `residual = -0.70`
- `mixed = -0.40` (well within ±0.95, no limiting needed)
- Old formula: `headroom = 0.95 - 0.30 = 0.65`, `res_abs = 0.70 > 0.65` → **falsely triggers scaling**
- Output: `0.30 + (-0.70 × 0.65/0.70) = -0.35` instead of the correct `-0.40`

**Worse example:**
- `dry = 0.30`, `residual = -1.30`
- `mixed = -1.00` (should limit to -0.95)
- Old formula: `headroom = 0.65`, output = `0.30 - 0.65 = -0.35`
- Correct: output should be `-0.95` (error magnitude: 0.60)

## Fix: Directional headroom

The corrected formula computes available headroom based on the residual's direction:

```rust
let max_res = if residual >= 0.0 {
    (limit - dry).max(0.0)    // distance to +limit
} else {
    (limit + dry).max(0.0)    // distance to -limit
};
```

This ensures:
- Same-sign case: `dry = 0.80, res = 0.20` → `max_res = 0.95 - 0.80 = 0.15` (correctly limits)
- Opposite-sign case: `dry = 0.30, res = -0.70` → `max_res = 0.95 + 0.30 = 1.25` (correctly allows, no scaling)
- Opposite-sign overflow: `dry = 0.30, res = -1.30` → `max_res = 1.25`, output = `0.30 - 1.25 = -0.95` (correctly limits)

Applied to both Phase 1a (frequency-domain residual) and Phase 1b (time-domain residual) in `m6_mixer/mod.rs`.

## Diagnostic Infrastructure

A `HeadroomLog` system was added behind `#[cfg(feature = "headroom-log")]` to collect real-time peak data from the APO runtime:

- Per-block tracking: dry peak, residual peak, pre/post true_peak_guard peaks, TPG trigger status
- Headroom exceed entries with per-sample detail
- Flushed to `C:\ProgramData\Phaselith\headroom.log` every 100 blocks (~1 second)

This logging confirmed that the original headroom-proportional approach (without direction fix) already eliminated the hard-clipping distortion, and that `true_peak_guard` was never triggering during normal playback.

## Files Changed

| File | Change |
|------|--------|
| `crates/dsp-core/src/modules/m6_mixer/mod.rs` | Directional headroom formula, HeadroomLog diagnostics |
| `crates/apo-dll/Cargo.toml` | Enable `headroom-log` feature |

## Verification

- APO and Chrome extension both produce consistent output
- No distortion on previously-problematic loud passages
- HeadroomLog data confirms correct limiting behavior
