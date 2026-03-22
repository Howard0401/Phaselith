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
2. macOS initializes the browser engine with `max_sub_block = 1` by default
3. macOS also exposes a popup override between `1` and `8`

This is now a limited product-facing control on macOS only.

### Why This Exists

Recent listening and code review showed that lower sub-block sizes do sound
better in this browser runtime.

At the same time, they also increase the chance of host-side instability on
some macOS Chrome systems even when startup logs look healthy.

The real-world listening result is:

1. `1` sounds best
2. some macOS systems can still run `1` cleanly
3. other macOS systems need a much more conservative fallback
4. Windows has so far tolerated `1` well enough to keep it as the fixed default

### Why macOS Exposes `1 / 8`

macOS now exposes only two browser-facing choices in the popup:

1. `1`
   best sound quality, lowest sub-block, highest risk on unstable systems
2. `8`
   intentionally conservative fallback for users who hear crackle or stutter

This was chosen for a product reason, not just an engineering reason:

1. most users either want the best-sounding mode
2. or they want an obvious "make it stable" escape hatch
3. intermediate values are useful for engineering, but not necessary in the
   main user-facing control

This should still be understood as a runtime tradeoff, not as proof that macOS
or Chrome has a platform bug. It is a browser-host stability control.

## Transient Repair Note

Browser transient repair must follow analysis hop timing, not raw host callback timing.

The browser runtime can run very small callback quanta, so any time-domain shaping that is stamped onto every callback block can become audible as runtime artifact instead of useful repair.

The current browser path therefore relies on hop-aware transient gating in the core engine rather than per-callback pre-echo shaping.

macOS Chrome now defaults to a browser-safe transient mode that applies delayed pre-echo shaping only to the enhancement delta, not the dry waveform. Conservative rollback modes still exist in the extension if live listening ever exposes new artifacts.

That transient-safe mode is now paired with the macOS browser sub-block policy
above. In practice, stable macOS browser playback currently depends on both:

1. delta-only delayed transient shaping
2. the ability to fall back from `1` to a larger sub-block when a given macOS
   Chrome system cannot sustain the best-sounding mode

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
