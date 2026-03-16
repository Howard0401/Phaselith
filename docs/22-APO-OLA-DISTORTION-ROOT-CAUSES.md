# APO OLA Distortion — Root Causes and Fixes

This document catalogs all distortion ("爆音") causes found during the APO Standard-mode migration, their root causes, and fixes. Purpose: prevent future recurrence.

## Cause 1: M6 non-directional headroom formula

**Symptom**: Distortion during loud passages, audible as harsh high-frequency harmonics.

**Root cause**: The residual limiting formula `headroom = limit - |dry|` does not account for the sign relationship between dry and residual. When they have opposite signs, the available space is `limit + |dry|`, not `limit - |dry|`. This causes:
- False triggering: residual gets scaled down when the mixed signal is actually within limits
- Incorrect scaling magnitude: when limiting IS needed, the wrong headroom value produces wrong output levels

**Fix**: Directional headroom — compute available space based on residual direction:
```rust
let max_res = if residual >= 0.0 {
    (limit - dry).max(0.0)    // distance to +limit
} else {
    (limit + dry).max(0.0)    // distance to -limit
};
```

**How to avoid**: Any gain-staging formula that computes "available headroom" must consider the direction of the signal being limited relative to the reference signal.

## Cause 2: M6 per-sample hard clamp

**Symptom**: Flat-topped waveforms during loud passages — classic hard clipping with audible high-frequency harmonics.

**Root cause**: After mixing dry + residual with headroom limiting, a final `.clamp(-limit, limit)` was applied per-sample. When `dry` itself exceeded the limit (e.g., dry = 0.9999, limit = 0.95), the clamp hard-cut the signal to 0.95. Consecutive samples all getting clipped to 0.95 produces flat-topped waveforms.

**Fix**: Remove per-sample `.clamp()`. Rely on `true_peak_guard` for block-level uniform gain reduction, which preserves waveform shape by scaling all samples equally.

**How to avoid**: Never use per-sample hard clamps on audio signals. Use block-level uniform gain reduction (limiter/compressor pattern) to preserve waveform shape.

## Cause 3: M5 OLA double-add (multi-hop)

**Symptom**: Energy doubling in overlap regions, audible as amplitude modulation artifacts and distortion.

**Root cause**: When APO block_size (528) > hop_size (256), M0's FrameClock reports `hops_this_block = 2`. But M2 produces only one STFT analysis per `process()` call. The same ISTFT frame was added to the OLA buffer twice at different positions, causing signal energy to double in overlap regions.

An intermediate fix (`advance_write_only`) avoided double-add by advancing the write pointer without accumulating data, but this produced amplitude dips because the "empty" hops contained only overlap tails from prior frames.

**Fix**: Engine sub-block processing — split blocks larger than hop_size into sub-blocks of ≤ hop_size samples before passing through the module chain. This guarantees `hops_this_block ≤ 1` per sub-block, ensuring 1:1 correspondence between M2 analysis and M5 OLA synthesis.

**How to avoid**: The audio engine must guarantee that the number of STFT analyses matches the number of OLA synthesis frames. When the host block size is not aligned with the hop size, the engine layer must handle the mismatch — individual modules should not need to work around it.

## Cause 4: APO block > hop structural constraint

**Symptom**: All multi-hop artifacts (Cause 3) stem from this architectural mismatch.

**Root cause**: Windows Audio Engine sends blocks of variable size (typically 480-528 samples at 48kHz) determined by the audio endpoint, not by our DSP parameters. With Standard mode (hop = 256), a single block can span 2+ hop boundaries. The module chain (M0-M7) was designed assuming at most one hop per process() call.

**Fix**: The engine's `process()` method now splits large blocks:
```rust
if samples.len() <= hop {
    self.process_sub_block(samples);  // fast path (Chrome/WASM)
} else {
    // split into ≤ hop_size sub-blocks
    while offset < samples.len() {
        let sub_len = hop.min(samples.len() - offset);
        self.process_sub_block(&mut samples[offset..offset + sub_len]);
        offset += sub_len;
    }
}
```

**How to avoid**: Any host integration must ensure the DSP pipeline sees blocks ≤ hop_size. This is an engine-level concern, not a module-level one.

## Files Changed

| File | Change |
|------|--------|
| `crates/dsp-core/src/engine.rs` | Sub-block processing: split `process()` into sub-block loop + `process_sub_block()` |
| `crates/dsp-core/src/modules/m5_reprojection/mod.rs` | Remove `advance_write_only` workaround, restore simple `add_frame` loop |
| `crates/dsp-core/src/modules/m5_reprojection/overlap_add.rs` | Remove `advance_write_only()` method |
| `crates/apo-dll/src/apo_impl.rs` | Switch from `UltraExtreme` to `Standard` QualityMode |
| `crates/dsp-core/src/modules/m6_mixer/mod.rs` | Directional headroom formula (committed in v0.1.2) |

## Safety Analysis: Sub-block Processing

- **M0-M5**: Do not modify `samples[]` — they read input and write to `ProcessContext` fields. Sub-block slicing is transparent.
- **M6**: First module to write `samples[i] = dry + scaled_res`. Each sub-block's slice is independent.
- **IIR filters** (M6 `hf_deemph`): State carries across sub-blocks naturally (filter state is in the module, not in samples).
- **M5 OLA drain FIFO**: Accumulates across sub-blocks correctly — write position persists in module state.
- **Timing**: `processing_time_us` accumulates with `+=` across sub-blocks.
- **Chrome/WASM**: Block (128) ≤ hop (256), takes fast path — zero behavior change.
