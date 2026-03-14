<p align="center">
  <img src="chrome-ext/icons/cirrus-logo.svg" width="128" alt="CIRRUS Logo">
</p>

<h1 align="center">CIRRUS</h1>

<p align="center">
  <strong>English</strong> | <strong><a href="README.zh-TW.md">繁體中文</a></strong> | <strong><a href="README.zh-CN.md">简体中文</a></strong>
</p>

CIRRUS = Constrained Inverse Restoration with Real-time Uncertainty and Reprojection Solver

CIRRUS is a real-time perceptual audio restoration and presentation engine aimed at making damaged, lossy, spatially collapsed, or harsh playback sound more focused, more intelligible, and more physically "placed" in front of the listener.

This repository contains the current CIRRUS core, a browser runtime, a Windows APO runtime, and the control-plane code needed to turn the DSP into a real product rather than a lab demo.

## Project Purpose

CIRRUS exists to answer a simple question:

How far can software push everyday playback toward premium-system presentation without turning into a gimmick, a fixed EQ curve, or a one-trick spatial effect?

The project focuses on five goals:

1. Reduce masking, haze, harshness, and codec-like roughness.
2. Strengthen center image and front-focused source placement.
3. Preserve musical density and impact instead of flattening everything into "clarity."
4. Work on ordinary playback chains, including browsers, laptop speakers, headphones, and system-wide desktop audio.
5. Stay explainable, testable, and shippable as an engineering system.

## CIRRUS Is Not Just EQ

EQ applies a fixed or semi-fixed tonal curve.

CIRRUS instead:

1. Estimates what kind of damage or collapse is present.
2. Decomposes the signal into harmonic, air, transient, spatial, and phase-related structure.
3. Builds residual candidates that may restore missing or suppressed perceptual structure.
4. Self-validates those residuals through a reprojection step instead of blindly adding them back.
5. Mixes the result through a safety layer that protects peaks, low-band stability, and long-term listenability.

That is why CIRRUS can sound like it reduces muddiness, restores front focus, or improves image lock even when it is not acting like a conventional EQ.

## Core Algorithm

The current engine is organized as M0-M7:

1. `M0 Orchestrator`
   Buffers host callbacks, manages frame and hop timing, and provides aligned analysis windows.
2. `M1 Damage Posterior`
   Estimates cutoff, clipping, limiting, stereo collapse, and confidence.
3. `M2 Tri-Lattice`
   Produces the analysis lattices that later modules read.
4. `M3 Factorizer`
   Separates harmonic, air, transient, and spatial fields.
5. `M4 Inverse Residual Solver`
   Generates candidate repairs in frequency and time domains.
6. `M5 Self-Reprojection Validator`
   Rejects or shrinks repairs that do not survive a degradation-consistency test.
7. `M6 Perceptual Safety Mixer`
   Mixes validated residuals with loudness compensation, character shaping, and ambience-preserve safeguards.
8. `M7 Governor`
   Publishes telemetry and runtime state for control and debugging.

Detailed algorithm notes live in [docs/03-CORE-ALGORITHM.md](docs/03-CORE-ALGORITHM.md).

## Runtime Surfaces

### Browser Runtime

- Path: `chrome-ext -> AudioWorklet -> wasm-bridge -> dsp-core`
- Best for: immediate listening validation, browser media playback, fast shipping
- Current status: the most mature stereo listening runtime in this repository

### Windows Desktop Runtime

- Path: `Tauri control panel -> mmap IPC -> APO DLL -> dsp-core`
- Best for: system-wide playback on Windows
- Current status: usable and strategically important, but still a transitional stereo runtime rather than the final flagship architecture

### Future Runtime Targets

- macOS: Core Audio based runtime
- Linux: PipeWire based runtime

Roadmap and limitations live in [docs/08-ROADMAP-AND-LIMITATIONS.md](docs/08-ROADMAP-AND-LIMITATIONS.md).

## Repository Layout

- `chrome-ext/`
  Browser extension runtime and AudioWorklet host.
- `crates/dsp-core/`
  CIRRUS algorithm core.
- `crates/wasm-bridge/`
  WASM bridge used by the browser runtime.
- `crates/apo-dll/`
  Windows APO runtime.
- `crates/tauri-app/`
  Desktop control panel and APO management UI.
- `docs/`
  Numbered architecture, algorithm, runtime, validation, and licensing documentation.

Note: crate names still use the historical `asce-*` prefix internally. The product and architecture identity documented here is `CIRRUS`.

## Documentation Map

Start with these files:

1. [docs/00-DOCUMENT-MAP.md](docs/00-DOCUMENT-MAP.md)
2. [docs/01-PROJECT-SCOPE.md](docs/01-PROJECT-SCOPE.md)
3. [docs/03-CORE-ALGORITHM.md](docs/03-CORE-ALGORITHM.md)
4. [docs/04-BROWSER-RUNTIME-FLOW.md](docs/04-BROWSER-RUNTIME-FLOW.md)
5. [docs/05-WINDOWS-APO-RUNTIME-FLOW.md](docs/05-WINDOWS-APO-RUNTIME-FLOW.md)
6. [docs/10-DEFENSIVE-PUBLICATION.md](docs/10-DEFENSIVE-PUBLICATION.md)
7. [docs/11-CHROME-STORE-LAUNCH-AND-MONETIZATION.md](docs/11-CHROME-STORE-LAUNCH-AND-MONETIZATION.md)
8. [docs/12-PUBLIC-V0.1-OPEN-SOURCE-PACKAGING.md](docs/12-PUBLIC-V0.1-OPEN-SOURCE-PACKAGING.md)

## Status

> **Early Preview** — CIRRUS is under active development. The core algorithm is stable and producing strong listening results, but APIs, configuration options, and runtime architecture may change without notice.

Today, the browser runtime is the cleanest listening reference in this repository.

The Windows APO path is already valuable, but it is still documented as a transitional runtime because its stereo execution model is not yet the final stereo-native design.

That distinction matters:

- It keeps documentation honest.
- It protects the strongest current listening result.
- It prevents architectural debt from being mistaken for finished flagship design.

## Licensing

This repository is licensed under the GNU Affero General Public License v3.0 or later.

- See [LICENSE](LICENSE) for the full license text.
- See [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md) for commercial licensing options.
- See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution terms and contributor licensing rules.
- See [docs/10-DEFENSIVE-PUBLICATION.md](docs/10-DEFENSIVE-PUBLICATION.md) for the defensive-publication-style technical disclosure.

Commercial licensing is available for teams that want to use, embed, or redistribute CIRRUS without AGPL obligations.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=Howard0401/CIRRUS&type=Date)](https://star-history.com/#Howard0401/CIRRUS&Date)
