# Contributing to Phaselith

Contributions are welcome, but this project is maintained with a dual-track licensing strategy:

1. The public repository is licensed under `AGPL-3.0-or-later`.
2. Howard Chen, as Project Owner, may also ship commercial licenses for the same codebase.

If you are comfortable contributing under those terms, you are welcome here.

## How to Contribute

1. Open an issue or discussion before large architecture changes.
2. Keep patches focused.
3. Include tests whenever behavior changes.
4. Do not weaken the core listening goals:
   - stable front image
   - strong center lock
   - low roughness
   - premium-system presentation
5. Keep real-time paths allocation-free.

## Development Expectations

Before opening a pull request:

1. Run Rust tests for the affected crates.
2. Explain the listening or runtime problem the change is solving.
3. Call out any risk to imaging, ambience, punch, or harshness control.
4. Update docs if the change affects architecture, controls, runtime behavior, or licensing.

## Contribution Licensing Terms

By submitting a patch, pull request, issue attachment, or any other contribution to this repository, you agree that:

1. You have the legal right to submit the contribution.
2. Your contribution may be distributed under `AGPL-3.0-or-later` as part of this repository.
3. You additionally grant Howard Chen, as Project Owner, a perpetual, worldwide, irrevocable, non-exclusive, sublicensable right to use, modify, sublicense, relicense, and commercialize your contribution as part of Phaselith and related products.
4. Your contribution may be included in paid, proprietary, or commercially licensed editions of Phaselith.
5. Attribution may be preserved through Git history, release notes, contributor records, or a contributors list when maintained.

This policy is here to keep the commercial-license path viable without blocking community collaboration.

If you cannot agree to those terms, please do not submit code to this repository.

For substantial external contributions, the maintainer may additionally ask contributors to confirm the terms in [CLA.md](CLA.md) before merge.

## Ownership and Stewardship

This is an owner-led project.

- Howard Chen, as Project Owner, retains final authority over roadmap, merges, licensing, branding, and commercial distribution.
- Contributions are welcome and credited, but the repository is not governed as a community-owned project.

## Code Areas

- `crates/dsp-core/`
  Phaselith core DSP logic
- `crates/wasm-bridge/`
  Browser runtime bridge
- `chrome-ext/`
  Chrome extension host and AudioWorklet runtime
- `crates/apo-dll/`
  Windows system-wide runtime
- `crates/tauri-app/`
  Desktop control plane

## Good Contribution Areas

- Tests and regression coverage
- Runtime correctness fixes
- Documentation improvements
- Measurement and validation tooling
- Platform adapters that preserve the current listening signature

## Changes That Need Extra Caution

- Anything touching M5 or M6
- Stereo semantics changes
- Loudness compensation changes
- Tail preserve / ambience behavior
- FFT/OLA runtime replacement work

These areas can improve the system dramatically, but they can also destroy the strongest parts of the current sound if changed casually.
