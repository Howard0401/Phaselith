# Phaselith VST3 Plugin

DAW plugin wrapping the same DSP core used by APO and Chrome Extension.

## Build

```bash
# Windows
cargo build -p phaselith-vst3 --release

# macOS (Apple Silicon)
cargo build -p phaselith-vst3 --release --target aarch64-apple-darwin
```

## Install

### Windows

```
mkdir "C:\Program Files\Common Files\VST3\Phaselith.vst3\Contents\x86_64-win"
copy target\release\phaselith_vst3.dll "C:\Program Files\Common Files\VST3\Phaselith.vst3\Contents\x86_64-win\phaselith_vst3.vst3"
```

### macOS (Apple Silicon)

```bash
mkdir -p ~/Library/Audio/Plug-Ins/VST3/Phaselith.vst3/Contents/MacOS
cp target/aarch64-apple-darwin/release/libphaselith_vst3.dylib \
   ~/Library/Audio/Plug-Ins/VST3/Phaselith.vst3/Contents/MacOS/Phaselith
```

## DAW Setup

1. Open your DAW (Studio One, Cubase, Ableton, etc.)
2. Scan for new plugins
3. "Phaselith" appears under Effects → Restoration / Mastering
4. Insert on any track or master bus

## Parameters

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Strength | 0-100% | 70% | Overall restoration intensity |
| HF Reconstruction | 0-100% | 80% | High-frequency detail restoration |
| Dynamics | 0-100% | 60% | Dynamic range restoration |
| Transient | 0-100% | 50% | Transient/attack repair |
| Ambience | 0-30% | 0% | Tail/reverb preservation |
| Style | Reference/Warm/Bass+ | Reference | Sonic character preset |
| Quality | Light/Standard/Ultra/Extreme | Standard | CPU vs quality tradeoff |
| Enabled | On/Off | On | Bypass toggle |

## License

- Free: Strength capped at 50%, Reference preset only
- Pro ($39 one-time): Full controls, all presets

## Architecture

Dual mono engines (L/R) — same pattern as APO and Chrome Extension.
Each channel processed independently to prevent phase bleed.
Cross-channel context shared with one-frame delay for stereo awareness.
