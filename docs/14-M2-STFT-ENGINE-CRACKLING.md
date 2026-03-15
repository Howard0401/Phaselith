# 14. M2 StftEngine Crackling Issue

## Summary

Switching M2 (TriLatticeAnalysis) from legacy per-call `analyze_lattice()` to
pre-allocated `StftEngine::analyze()` introduces audible crackling artifacts in
the Chrome extension runtime on Windows.

## Timeline

- Commit `1454f81` (2026-03-14 09:26): last known-good WASM build (339 KB).
- Commit `1072113` (2026-03-14 10:26): introduced `StftEngine` in M2 — first
  crackling build (347 KB).
- Confirmed via binary search: reverting only M2's `process()` to use legacy
  `stft::analyze_lattice()` eliminates the crackling while keeping all other
  changes from `1072113` (M0 FrameClock, M5 pilot ISTFT, M6 EMA reorder).

## What Changed

Before (`5da0546`, good):
```rust
stft::analyze_lattice(&self.micro_scratch[..N], &mut ctx.lattice.micro, self.sample_rate);
```

After (`1072113`, bad):
```rust
if let Some(engine) = &mut self.micro_engine {
    engine.analyze(&self.micro_scratch[..N], &mut ctx.lattice.micro);
}
```

Both paths use the same Hann window and `rustfft` forward FFT, and unit tests
confirm magnitude/phase agreement to < 1e-6. The crackling is therefore not a
correctness bug visible in single-frame tests.

## Likely Root Cause (Hypothesis)

The legacy path allocates a fresh `FftPlanner` + complex buffer each call and
immediately frees them. The `StftEngine` path pre-allocates three engines
(256 / 1024 / 2048 FFT sizes) with persistent `Vec<Complex<f32>>` buffers and
`Arc<dyn Fft>` plan objects.

In the Chrome AudioWorklet WASM runtime (128-sample render quantum, ~2.67 ms
budget at 48 kHz), the extra persistent heap pressure from three pre-allocated
engines may cause:

1. **WASM linear memory growth** at init that pushes the heap into a larger
   page configuration, triggering occasional GC pauses or allocation stalls
   during real-time processing.
2. **Cache pressure**: three persistent complex buffers (256 + 1024 + 2048 =
   3328 × 8 = 26 KB) plus windows (13 KB) plus FFT plan internals compete
   with the audio processing hot path for L1/L2 cache.
3. **Allocator fragmentation**: the legacy per-call allocate/free pattern may
   paradoxically work better in WASM's `dlmalloc` because it reuses the same
   heap region every call, while the pre-allocated pattern spreads objects
   across memory.

None of these are confirmed. The crackling does not reproduce in native Rust
tests, only in the browser AudioWorklet runtime.

## Current Workaround

The Chrome extension ships the old WASM binary (`asce_wasm_bridge.wasm` from
2026-03-14, renamed to `phaselith_wasm_bridge.wasm`). This binary uses the
legacy `analyze_lattice()` path.

## Rules for Future Work

1. **Do not switch M2 to StftEngine in production WASM** until the crackling
   root cause is confirmed and fixed.
2. **Any M2 FFT path change must be validated by listening in the Chrome
   extension**, not just unit tests. Unit tests cannot reproduce the artifact.
3. **The legacy `analyze_lattice()` free function must remain available** as
   the proven production path.
4. When investigating further, focus on:
   - WASM memory layout and allocation patterns
   - AudioWorklet timing budget (measure `process()` duration)
   - Whether the crackling correlates with GC or memory growth events
   - Whether a single-engine variant (only core lattice) also crackles
