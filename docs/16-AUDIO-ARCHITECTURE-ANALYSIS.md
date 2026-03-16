# 16. Audio Architecture Analysis & Technology Selection

## Purpose

This document records the technical analysis of Windows audio pipeline architectures,
explains why APO was chosen as the primary delivery mechanism for Phaselith, and
documents the HF de-emphasis anti-sibilance system added to M6.

---

## 1. Windows Audio Pipeline: Where APO Sits

```
Application (Chrome, Spotify, etc.)
    │
    ▼
Windows Audio Session API (WASAPI Shared Mode)
    │
    ▼
Windows Audio Engine (audiodg.exe)
    ├── Sample Rate Conversion (SRC) ← 44.1→48kHz happens here
    ├── Mixing (all app streams merged)
    ├── SFX Insert Point ← ★ Phaselith APO runs here ★
    │       Format: 24-bit float, 48 kHz (device setting)
    │       Block size: typically 480 samples (10ms)
    │       Priority: MMCSS real-time thread
    ▼
DAC / Audio Driver
    │
    ▼
Speakers / Headphones
```

### Key observations

- The APO receives **post-mixer, post-SRC** audio. The signal is already at
  the device sample rate (typically 48 kHz) and 24-bit float precision.
- The APO does **NOT** bypass the audio driver. It runs inside `audiodg.exe`
  as part of the Windows Audio Engine's signal chain.
- SRC occurs **before** the APO insertion point, so the APO always sees
  the device-native sample rate.

---

## 2. Architecture Comparison

### 2.1 APO (Audio Processing Object) — Current choice

| Aspect | Detail |
|--------|--------|
| **Scope** | System-wide: all applications automatically enhanced |
| **User setup** | Install once, zero per-app configuration |
| **Signal quality** | 24-bit float, post-mixer (clean, no additional resampling) |
| **Latency** | Zero additional latency (in-place processing on RT thread) |
| **Limitation** | Cannot change sample rate; processes at device rate |
| **Best for** | Streaming music, games, video — "set and forget" enhancement |

### 2.2 WASAPI Exclusive Mode

| Aspect | Detail |
|--------|--------|
| **Scope** | Single application only; other apps go silent |
| **Signal quality** | Bit-perfect to DAC (no SRC, no mixer) |
| **User setup** | Each app must be configured individually |
| **Limitation** | Monopolizes audio device; incompatible with multitasking |
| **Best for** | Audiophile local playback (hi-res FLAC at native DAC rate) |

### 2.3 ASIO (Audio Stream Input/Output)

| Aspect | Detail |
|--------|--------|
| **Scope** | Single application only; requires ASIO driver |
| **Signal quality** | Bit-perfect, low-latency |
| **User setup** | Requires special driver (ASIO4ALL or vendor ASIO) |
| **Limitation** | Not all DACs support it; adds driver dependency |
| **Best for** | Professional audio production (DAWs, recording) |

### 2.4 Chrome Extension (Web Audio API)

| Aspect | Detail |
|--------|--------|
| **Scope** | Per-browser-tab |
| **Signal quality** | Web Audio API adds extra resampling + mixing layer |
| **User setup** | Install extension, per-tab toggle |
| **Limitation** | Extra resampling degrades quality; WASM performance ceiling |
| **Best for** | Cross-platform (macOS/Linux), users who cannot install APO |

---

## 3. Why APO is the Right Choice

1. **Widest coverage**: Every application benefits automatically — Chrome,
   Spotify, games, video players — without any per-app configuration.

2. **Signal quality is excellent**: The post-mixer signal is 24-bit 48 kHz,
   which is already high quality. For streaming music (lossy 44.1/48 kHz),
   the Windows SRC impact is negligible.

3. **Zero user friction**: Install once, it just works. This aligns with
   Phaselith's product vision of "invisible enhancement."

4. **No conflict with other apps**: Unlike WASAPI Exclusive/ASIO, other
   applications continue to produce sound normally.

5. **ASIO/WASAPI Exclusive would be a different product entirely**: Those
   architectures only make sense for a dedicated standalone player, which
   is a fundamentally different product category.

### When WASAPI Exclusive / ASIO would matter

- Playing local hi-res files (96 kHz+ FLAC/DSD) on a DAC with a native
  rate different from the source — bypassing Windows SRC preserves the
  original sample rate.
- Professional audio production requiring sub-1ms latency.
- These are v2.0+ features that would require a standalone player UI.

---

## 4. Future: Algorithmic Upsampling via APO

The APO architecture naturally supports future algorithmic upsampling:

1. User sets Windows audio device to **96 kHz** (in Sound Settings).
2. Windows SRC upsamples 44.1/48 kHz source → 96 kHz.
3. APO receives 96 kHz signal with **empty spectrum above 24 kHz**
   (original Nyquist limit).
4. **M2 (HF Reconstruction)** detects the ~24 kHz cutoff and synthesizes
   harmonic content in the 24–48 kHz range.

### Required changes for upsampling support

- **Larger FFT**: 96 kHz with 1024-point FFT gives ~94 Hz/bin resolution.
  Need 2048 or 4096 for adequate frequency resolution.
- **CPU budget doubles**: Twice the sample rate = twice the processing.
- **M2 algorithm upgrade**: Current harmonic synthesis targets codec cutoffs
  (typically 16–20 kHz). Upsampling requires spectral band replication (SBR)
  or similar technique for the 24–48 kHz range.

No architectural changes needed — the APO framework already handles arbitrary
device sample rates. Only the DSP algorithms need adaptation.

---

## 5. HF De-emphasis Anti-Sibilance System

### Problem

At high Compensation Strength (and high Transient Repair), some songs produce
audible sibilance/harshness — artificial high-frequency content in the 6–10 kHz
range. This is the restoration residual's HF energy being amplified beyond
what's perceptually natural.

### Root cause

The restoration residual from M5 (self-reprojection) contains broadband content.
When mixed at high gain (strength → 1.0), the HF portion becomes audible as
sibilance. The pre-echo suppression in M3 can also introduce sharp gain
transitions that create HF artifacts.

### Solution: 1-pole LP shelf filter on residual

Applied in M6 `PerceptualSafetyMixer` at three mixing points:

1. **Phase 1a** — Frequency-domain validated residual
2. **Phase 1b** — Time-domain residual (declipping)
3. **Transient delta emit** — Delayed transient repair output

#### Algorithm

```
For each residual sample r[n]:
    LP state += alpha * (r[n] - LP state)     // 1-pole LP tracker
    HP = r[n] - LP state                       // high-pass component
    output = r[n] - shelf_amount * HP          // attenuate HP portion
```

#### Parameters

| Parameter | Value | Purpose |
|-----------|-------|---------|
| Crossover frequency | 4 kHz | LP -3dB point; sibilance starts ~4 kHz |
| Shelf amount | `strength * 0.85` | Linear scaling; more aggressive at high strength |
| Alpha (48 kHz) | 0.408 | `1 - exp(-2π × 4000 / 48000)` |

#### Measured attenuation (strength = 1.0, 48 kHz)

| Frequency | Gain | Attenuation |
|-----------|------|-------------|
| 500 Hz | ~0.99 | -0.1 dB (transparent) |
| 2 kHz | ~0.91 | -0.8 dB (barely noticeable) |
| 8 kHz | ~0.53 | -5.5 dB (significant) |
| 16 kHz | ~0.35 | -9.1 dB (heavy) |

#### Design rationale

- **Shelf (not LP)**: A pure LP would dull the entire residual. The shelf
  approach passes low/mid frequencies unchanged and only attenuates HF.
- **Scales with strength**: At low strength, the residual is quiet enough
  that sibilance isn't audible. At high strength, aggressive HF reduction
  is needed. Linear scaling (not quadratic) ensures adequate attenuation
  at moderate strength levels.
- **4 kHz crossover**: Lower than initially tested 6 kHz. Sibilance energy
  starts around 4–5 kHz; the lower crossover ensures the shelf is already
  active in the sibilance onset region.
- **Per-sample state**: The LP filter state persists across blocks for
  smooth continuity. Separate states for each mixing path prevent cross-talk.

---

## 6. APO vs Chrome Extension Parameter Differences

The Tauri control panel exposes 4 sliders + 3 selects for the APO:

| APO Parameter | Chrome Extension | Difference |
|--------------|------------------|------------|
| Compensation Strength | Strength | Same: multiplicative scaling of all restoration |
| HF Reconstruction | HF Reconstruction | Same parameter, but APO signal rarely needs it |
| Dynamics Restoration | Dynamics | Same: only active when clipping detected (>5%) |
| Transient Repair | Transient | Same: pre-echo suppression via spectral flux |
| Quality Preset | (fixed Standard) | APO allows Light/Standard/Ultra |
| Phase Mode | (fixed Linear) | APO allows Linear/Minimum phase |
| Synthesis Mode | (fixed Legacy) | APO defaults to FftOlaPilot (better for APO blocks) |

### Why HF Reconstruction shows "0 effect" on APO

M1 (damage estimator) only activates HF reconstruction when it detects a
frequency cutoff below ~20 kHz. High-quality streaming audio (Spotify Premium
320 kbps, YouTube 256 kbps AAC) typically has no audible cutoff — the codec
preserves content up to Nyquist. Therefore `hf_reconstruction` parameter
has no effect on these sources.

### Why Dynamics shows "0 effect"

M1 only activates dynamics restoration when it detects clipping exceeding
~5% of peak samples. Well-mastered streaming audio rarely clips, so the
`dynamics` parameter has no effect.

### Why Transient Repair has the most effect

Pre-echo suppression operates independently of damage detection. It uses
spectral flux analysis to detect transient events and suppress pre-echo
energy in the preceding hop. This works on any audio source regardless
of compression quality, which is why it's the most consistently audible
parameter.
