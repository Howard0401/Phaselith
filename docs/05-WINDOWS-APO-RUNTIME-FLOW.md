# 05. Windows APO Runtime Flow

## Purpose

The Windows APO path is CIRRUS's system-wide runtime strategy.

Its long-term value is much larger than the browser path because it can become the real flagship desktop playback engine.

## Data Flow

1. User installs and enables the APO.
2. Windows Audio Engine loads the APO DLL.
3. The APO processes system playback in real time.
4. A Tauri control panel communicates with the APO through shared memory.
5. `dsp-core` provides the actual DSP behavior.

## Main Files

- `crates/apo-dll/src/apo_impl.rs`
- `crates/apo-dll/src/format_negotiate.rs`
- `crates/apo-dll/src/mmap_ipc.rs`
- `crates/tauri-app/src/main.rs`
- `crates/tauri-app/src/commands.rs`
- `crates/tauri-app/src/ipc_bridge.rs`

## What the APO Path Is Today

Today, the APO path is:

1. strategically important
2. already useful
3. still architecturally transitional

That last point matters.

The current APO implementation is not yet the final stereo-native runtime because it still uses a mono-per-channel execution approach internally, with reset-based protection against cross-channel contamination.

## Why That Is Called Transitional

The current implementation exists because:

1. it provides a usable Windows system-wide route now
2. it avoids certain state contamination problems
3. it keeps the project moving while the final stereo-native design is still being defined

But it is not yet the final design because:

1. stereo semantics are not fully clean
2. it does not yet represent the intended flagship stereo runtime
3. future work should either move to a dual-engine shared-analysis model or a true interleaved stereo architecture

## Why Keep APO Anyway

Because APO is still the right Windows product direction.

The correct conclusion is not:

`do not use APO`

The correct conclusion is:

`APO is the right desktop destination, but the current implementation is an intermediate runtime on the way there`

## Product Positioning

Short term:

1. browser can serve as the cleanest listening reference
2. APO can serve as the Windows system-wide product path

Long term:

1. APO should become the flagship Windows runtime
2. browser remains the fastest validation and adoption surface
