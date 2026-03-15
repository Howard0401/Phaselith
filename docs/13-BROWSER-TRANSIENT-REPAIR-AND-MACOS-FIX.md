# 13. Browser Transient Repair and macOS Fix

## Purpose

This note explains:

1. why the browser transient artifact was much more obvious on macOS Chrome than on Windows
2. what was actually wrong in the browser/runtime interaction
3. what was fixed in core
4. why the shipping macOS browser runtime still keeps a conservative transient fallback
5. how the browser runtime now splits transient probing into independent pre-echo and declip paths

## Symptom

In the Chrome extension runtime:

1. Windows listening stayed subjectively acceptable
2. macOS Chrome produced obvious crackle or bursty roughness as soon as DSP touched transient repair
3. the artifact disappeared when transient repair was bypassed

The rest of the browser path was not the main failure:

1. tab capture worked
2. `AudioWorklet` timing stayed inside budget
3. WASM initialization was stable
4. the artifact tracked the transient/pre-echo path specifically

## Root Cause

The original issue lived in `M3: Structured Factorizer`.

`M3` was calling time-domain pre-echo suppression directly on the current host callback block. In a low-latency browser runtime, that is the wrong time reference.

The underlying mistake was:

1. transient detection was analysis/hop based
2. waveform shaping was callback-local
3. the callback block was sometimes treated as if it were the same thing as the analysis frame

That mismatch made the browser output behave like a periodic fade pattern instead of a transient repair.

Relevant files:

- `crates/dsp-core/src/modules/m3_factorizer/mod.rs`
- `crates/dsp-core/src/modules/m3_factorizer/transient.rs`

## Why Windows Sounded Better While macOS Broke

This remains an engineering inference from runtime behavior, not a claim that Chrome or macOS had a platform bug.

The logic error was cross-platform, but macOS exposed it more clearly.

The likely reasons are:

1. macOS Chrome was running a strict low-latency `AudioWorklet` path with 128-frame quanta
2. the Core Audio path made callback-shaped modulation easier to hear as crackle or roughness
3. the Windows path likely masked more of that behavior through different buffering, scheduling, or driver behavior

So Windows was not necessarily "correct".

It was simply less revealing for this particular timing error.

## Core Design Fix

The core fix now has two layers.

### 1. Hop-aware trigger in M3

`M3` no longer treats every browser callback as a valid place to run pre-echo suppression.

It first computes whether the current block actually represents a transient event by requiring:

1. `ctx.hops_this_block > 0`
2. real transient activity from the short-window field and/or spectral flux
3. non-trivial effective transient strength

That shared trigger logic now lives in:

- `crates/dsp-core/src/modules/m3_factorizer/transient.rs`

### 2. Hop-aligned delayed output repair path

The core now supports `delayed_transient_repair`.

That changes the repair timing model:

1. the current block is analyzed normally
2. if that block indicates a transient event, the engine applies pre-echo suppression to a hop-delayed output region
3. the repaired audio is emitted one hop later instead of mutating the live callback block

In other words, transient detection still happens on the current hop, but waveform shaping happens on a delayed output buffer instead of the live callback block.

This is implemented in:

- `crates/dsp-core/src/modules/m6_mixer/mod.rs`
- `crates/dsp-core/src/config.rs`
- `crates/wasm-bridge/src/lib.rs`

The browser runtime can enable that mode through:

- `chrome-ext/src/offscreen.js`
- `chrome-ext/src/worklet-processor.js`

### 3. Split transient controls

The core no longer has to treat `transient` as one indivisible browser toggle.

It now supports two independent transient scalars:

1. `pre_echo_transient_scaling`
2. `declip_transient_scaling`

That split matters because the browser artifact may come from more than one transient-driven stage:

1. pre-echo suppression in `M3` / delayed repair in `M6`
2. peak-estimation aggressiveness in `M4` declip

Relevant files:

- `crates/dsp-core/src/config.rs`
- `crates/dsp-core/src/modules/m3_factorizer/mod.rs`
- `crates/dsp-core/src/modules/m4_solver/mod.rs`
- `crates/dsp-core/src/modules/m6_mixer/mod.rs`
- `crates/wasm-bridge/src/lib.rs`

## Why This Design Is Safer

The delayed path is safer in browser runtimes because it respects the real timing problem.

It does not try to "guess better" with a smaller strength or tighter threshold.

Instead it changes where the waveform fade happens:

1. not on the block currently being rendered
2. not on every callback
3. only on a buffered block that is temporally behind the detected transient event

That is much closer to what pre-echo suppression actually wants semantically.

## Files Changed

Core:

- `crates/dsp-core/src/config.rs`
- `crates/dsp-core/src/modules/m3_factorizer/mod.rs`
- `crates/dsp-core/src/modules/m3_factorizer/transient.rs`
- `crates/dsp-core/src/modules/m6_mixer/mod.rs`
- `crates/wasm-bridge/src/lib.rs`

Browser runtime wiring:

- `chrome-ext/src/background.js`
- `chrome-ext/src/offscreen.js`
- `chrome-ext/src/worklet-processor.js`
- `chrome-ext/src/popup.js`
- `chrome-ext/manifest.json`

## Validation Status

Automated validation completed:

1. `cargo test -p asce-dsp-core`
2. targeted unit coverage for the new transient gate helper
3. targeted unit coverage for delayed transient shaping in `M6`
4. targeted unit coverage that `pre_echo_transient_scaling = 0` fully bypasses the delayed path
5. extension WASM rebuild via `scripts/build-extension.sh`

Live browser listening produced an important result:

1. disabling `transient` on macOS still removes the noise reliably
2. re-enabling transient with only the delayed M3-style pre-echo path was still not fully clean on the user's macOS Chrome setup
3. that means the audible problem is not just the old M3 callback-local fade
4. the browser runtime therefore still ships a conservative fallback by default while exposing hidden split probes for further isolation

The current investigation target is whether macOS browser noise comes from:

1. hop-aware pre-echo repair
2. declip peak-estimation aggressiveness
3. or an interaction between both

That is why the runtime now exposes independent browser-only probe modes instead of re-enabling the whole transient path at once.

## Practical Outcome

The current shipping browser behavior is:

1. Windows keeps the existing transient path and user-facing behavior
2. macOS Chrome now defaults to `transient-safe` in the extension
3. `transient-safe` keeps the delayed pre-echo path enabled, but only shapes the delayed enhancement delta and does not fade the dry waveform
4. `declip-safe` remains available as a conservative fallback that keeps declip transient scaling active while disabling pre-echo shaping
5. `stable-fallback` remains available as an escape hatch that forces `transient = 0`

The browser runtime also supports hidden macOS probe modes through `chrome.storage.local`:

1. `transient-safe`
   current default on macOS; enables delayed pre-echo shaping on the enhancement delta only
2. `declip-safe`
   conservative fallback; keeps declip transient scaling enabled while disabling pre-echo shaping
3. `stable-fallback`
   emergency fallback; forces `transient = 0`
4. `hop-probe`
   backward-compatible alias for `transient-safe`
5. `declip-probe`
   backward-compatible alias for `declip-safe`

## Next Engineering Step

The next correct step is not another threshold tweak.

That split now exists in code, and current real-device listening suggests the delayed delta-only pre-echo path is clean enough to serve as the new macOS default.

The next engineering step is:

1. keep validating `transient-safe` on longer real listening sessions
2. keep `declip-safe` and `stable-fallback` as instant rollback modes
3. if broader listening stays clean, keep the current naming and retire the old probe aliases later

Until one of those isolated paths proves clean in live listening, re-enabling the full transient setting on macOS browser playback is still too risky.
