# 15. APO and Chrome Extension Coexistence

## Discovery (2026-03-16)

When the Windows APO is installed system-wide, the Chrome extension produces audible crackling — even when the APO is in pure passthrough mode (no DSP, just memcpy).

Removing the APO via Tauri's Uninstall button immediately restores clean Chrome extension audio.

## Audio Path Analysis

When both runtimes are active, audio travels through two processing stages:

```
Tab audio
  → Chrome captures tab stream (getUserMedia)
  → AudioWorklet runs WASM DSP (Phaselith engine)
  → Chrome audio output
  → Windows Audio Engine (audiodg.exe)
  → APO (loaded as COM DLL in audiodg)
  → Audio driver → speakers
```

The APO adds a COM processing node inside audiodg.exe. Even when that node does nothing but copy input to output, its presence in the pipeline is enough to disrupt Chrome's audio timing.

## Why Passthrough Still Causes Crackling

The APO passthrough code is trivial (`output.copy_from_slice(input)`), but the COM infrastructure around it is not free:

1. **COM aggregation layer**: Every APOProcess call goes through IUnknown dispatch, RefCell borrow, catch_unwind wrapper
2. **DLL load side effects**: audiodg.exe loads the Rust DLL, initializing the Rust runtime, panic handler, and mmap IPC
3. **Thread scheduling**: Adding a non-trivial DLL to audiodg may change the OS scheduler's behavior for that process — affecting buffer completion deadlines for all audio streams, including Chrome's
4. **Buffer chain lengthening**: Even a zero-cost APO node adds one more buffer stage in the audio engine's processing graph, increasing end-to-end latency and jitter sensitivity

The Chrome AudioWorklet already operates near its timing budget (128 samples at 48kHz = 2.67ms). Any additional latency or jitter in the downstream path can cause the worklet to miss its render deadline, producing audible glitches.

## Constraint

This is not a bug to fix in the APO code — it is a fundamental architectural conflict. Two real-time audio processing stages in series (worklet + APO) have compounding timing constraints. The downstream stage's jitter affects the upstream stage's ability to meet deadlines.

## Solutions

### Option A: Mutual Exclusion (CHOSEN — Implemented)

Only one runtime is active at a time. The Tauri control panel manages this:

- **System-wide mode**: APO installed, Chrome extension disabled or not present
- **Per-tab mode**: Chrome extension active, APO not installed

The Tauri app UI should make this a clear mode selection, not a hidden conflict. When the user switches modes, Tauri installs/uninstalls the APO accordingly.

Pros: Simple, reliable, no DSP changes needed.
Cons: User must choose; cannot enhance both system audio and Chrome tabs simultaneously.

### Option B: APO Detects Pre-Processed Audio (Future)

The Chrome extension embeds an inaudible watermark or metadata flag in its output. The APO detects this marker and skips processing for that stream.

This solves double-processing but does NOT solve the COM overhead problem — the APO is still loaded in audiodg and still runs its process() callback, even if it immediately passes through.

### Option C: APO Per-Stream Bypass via Audio Engine Hints (Future, Complex)

Windows Audio Engine provides per-stream endpoint metadata. The APO could check if the audio source is Chrome and bypass itself. However:

- Windows does not reliably expose the source application to APOs
- This is undocumented/unsupported behavior
- Would not work for other browsers or media players with similar extensions

### Option D: Lightweight APO COM Layer (Future, Hard)

Minimize the COM overhead to the point where passthrough is truly zero-cost:

- Eliminate catch_unwind (trust the Rust code not to panic)
- Eliminate RefCell (use raw pointer with unsafe)
- Pre-initialize everything in LockForProcess, make process() a single function call
- Use `#[no_std]` to avoid Rust runtime overhead in the DLL

Even with all optimizations, the buffer chain lengthening effect may remain.

## Current Decision

**Option A (Mutual Exclusion)** is implemented. The Tauri app enforces two modes:

1. **Chrome Extension Mode** (default): Per-tab enhancement via browser. No APO installed. Badge shows green "Chrome Extension".
2. **System-Wide Mode**: APO installed for all system audio. Chrome extension must be disabled. Badge shows blue "System-Wide".

Implementation:
- Tauri UI shows mode badge based on IPC connection status (APO installed = System-Wide)
- Install APO shows confirm dialog warning user to disable Chrome extension first
- Uninstall APO reminds user they can re-enable Chrome extension
- Only one action button shown at a time (Install or Uninstall, not both)

The Chrome extension remains the primary listening reference and user-facing product. APO is the future flagship desktop runtime, to be developed separately once the zero-alloc DSP pipeline is complete (see doc 14).
