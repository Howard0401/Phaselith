# 02. Repository and Runtimes

## Repository Map

### `crates/dsp-core`

The algorithmic heart of CIRRUS.

Contains:

1. engine and shared process context
2. M0-M7 DSP modules
3. frame and overlap infrastructure
4. tests and integration checks

### `crates/wasm-bridge`

The WASM-facing adapter used by the browser runtime.

Responsibilities:

1. expose C-style entry points to the AudioWorklet
2. manage left/right engine instances
3. cache and apply runtime config updates
4. bridge browser audio blocks to `dsp-core`

### `chrome-ext`

Chrome extension host.

Responsibilities:

1. tab capture
2. offscreen orchestration
3. AudioWorklet lifecycle
4. popup controls

### `crates/apo-dll`

Windows APO implementation for system-wide audio.

Responsibilities:

1. format negotiation
2. real-time audio processing in `audiodg.exe`
3. shared-memory config and status bridge

### `crates/tauri-app`

Desktop control plane for the APO path.

Responsibilities:

1. UI and settings
2. install/uninstall helpers
3. control and status IPC

## Runtime Surfaces

### Browser Runtime

Data flow:

`tab capture -> offscreen document -> AudioWorklet -> wasm-bridge -> dsp-core`

Strengths:

1. fastest iteration loop
2. easiest listening validation
3. strongest current stereo listening reference in this repository

Current limitation:

- browser currently runs a dual-mono processing structure that preserves L/R layout but is not yet a fully stereo-native reconstruction path

### Windows APO Runtime

Data flow:

`system audio engine -> APO DLL -> dsp-core`

Strengths:

1. system-wide audio coverage
2. closest path to a flagship desktop product
3. natural home for future premium runtime features

Current limitation:

- APO is still a transitional stereo runtime and should not yet be described as the final stereo-native flagship architecture

## Why Both Paths Exist

The repository intentionally keeps both:

1. browser runtime for fast product iteration and listening validation
2. APO runtime for long-term system-wide flagship deployment

They share the same DSP core, but they do not yet share the same maturity level or stereo execution model.
