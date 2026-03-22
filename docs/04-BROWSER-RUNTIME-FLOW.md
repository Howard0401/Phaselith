# 04. Browser Runtime Flow

## Purpose

The browser runtime is currently the fastest path from algorithm change to real listening feedback.

It is also the cleanest reference runtime in the repository for day-to-day evaluation.

## Data Flow

1. User enables the extension in a browser tab.
2. Background logic obtains a tab capture stream.
3. The offscreen document owns the media graph and Worklet setup.
4. The AudioWorklet sends block data to the WASM bridge.
5. The WASM bridge calls into `dsp-core`.
6. Processed output returns to the same tab playback path.

## Main Files

- `chrome-ext/src/background.js`
- `chrome-ext/src/offscreen.js`
- `chrome-ext/src/worklet-processor.js`
- `crates/wasm-bridge/src/lib.rs`

## Current Stereo Reality

The browser runtime is not a mono collapse path.

It currently works like this:

1. left and right are processed separately
2. each side has its own engine instance
3. the original L/R layout is preserved

That means:

- it does not reduce the output to a single mono waveform
- but it is not yet the same thing as a full stereo-native reconstruction engine

In practice, the browser runtime today is best described as:

`dual-mono processing with preserved stereo layout`

## Why It Can Still Produce Strong Imaging

Even without full stereo-native reconstruction, listeners may still perceive much stronger front-focused imaging because:

1. each side becomes cleaner
2. masking is reduced
3. harshness and haze drop
4. center-common material is easier for the brain to localize as a phantom center

That can sound like the source "locks" in front of the listener even though the runtime is not yet doing the full stereo-native version of the algorithm.

## Current Strengths

1. fastest listening loop
2. strongest current stereo listening reference
3. easiest place to validate tab playback behavior

## Browser Sub-Block Policy

The browser runtime now uses a platform-specific sub-block policy when the
AudioWorklet initializes the WASM bridge:

1. Windows initializes the browser engine with `max_sub_block = 1`
2. macOS initializes the browser engine with `max_sub_block = 4`

This is not a product-facing control. It is a browser-runtime stability rule.

### Why This Exists

Recent listening and code review showed that the browser WASM bridge had been
forced to `with_max_sub_block(1)` for every platform.

That sounded very smooth, but it also meant a 128-sample Chrome render quantum
was split into 128 tiny engine passes per channel.

In the extension runtime, that has two different real-world outcomes:

1. Windows still subjectively tolerated the per-sample split
2. macOS Chrome became much more likely to audibly stutter even when there was
   no explicit `WASM_ERROR`, `RUNTIME_ERROR`, or early `overBudget` warning

So the browser runtime now treats `max_sub_block = 1` as a Windows-safe
listening mode, but not as the default macOS browser setting.

### Why macOS Uses 4 Instead of the Default Hop Path

The macOS browser setting is `4`, not the larger default hop-sized fast path,
because live listening showed a useful middle ground:

1. `1` sounded best but was not stable enough on macOS Chrome
2. `4` removed the obvious stutter while preserving more of the finer OLA
   readout character than a full rollback to the default fast path

This should be understood as a runtime tradeoff, not as proof that macOS or
Chrome has a platform bug. It is an engineering choice based on real listening
behavior in the browser host.

## Transient Repair Note

Browser transient repair must follow analysis hop timing, not raw host callback timing.

The browser runtime can run very small callback quanta, so any time-domain shaping that is stamped onto every callback block can become audible as runtime artifact instead of useful repair.

The current browser path therefore relies on hop-aware transient gating in the core engine rather than per-callback pre-echo shaping.

macOS Chrome now defaults to a browser-safe transient mode that applies delayed pre-echo shaping only to the enhancement delta, not the dry waveform. Conservative rollback modes still exist in the extension if live listening ever exposes new artifacts.

That transient-safe mode is now paired with the macOS browser sub-block policy
above. In practice, stable macOS browser playback currently depends on both:

1. delta-only delayed transient shaping
2. `max_sub_block = 4` instead of `1`

## Current Limitations

1. not yet true stereo-native reconstruction
2. depends on browser capture/runtime constraints
3. tab-follow and media-graph behavior still matter operationally

## What Should Not Be Claimed Yet

The browser runtime should not yet be described as:

1. final flagship stereo runtime
2. fully stereo-native reconstruction
3. equivalent to the intended future APO flagship path

It should be described as:

1. the current listening reference
2. the fastest shipping surface
3. the best current validation host for day-to-day sound decisions
