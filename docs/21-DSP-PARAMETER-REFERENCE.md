# 21 — DSP Parameter Reference

Complete reference of all tunable parameters across the Phaselith DSP pipeline.

---

## A. Global Engine Config (`config.rs` → `EngineConfig`)

| Parameter | Default | Range | Description |
|-----------|---------|-------|-------------|
| `strength` | 0.7 | 0.0–1.0 | Overall compensation strength (0 = bypass) |
| `hf_reconstruction` | 0.8 | 0.0–1.0 | High-frequency reconstruction intensity |
| `dynamics` | 0.6 | 0.0–1.0 | Dynamic range restoration intensity |
| `transient` | 0.5 | 0.0–1.0 | Transient repair intensity |
| `pre_echo_transient_scaling` | 1.0 | 0.0–1.0 | Transient effect on pre-echo suppression (0 disables) |
| `declip_transient_scaling` | 1.0 | 0.0–1.0 | Transient effect on declip peak estimation |
| `delayed_transient_repair` | false | bool | Pre-echo suppression on one-block delayed path |
| `phase_mode` | Linear | enum | Linear (quality) or Minimum (low latency) |
| `quality_mode` | Standard | enum | CPU budget selector |
| `ambience_preserve` | 0.0 | 0.0–1.0 | Dereverb compensation (start 0.05–0.15) |
| `synthesis_mode` | LegacyAdditive | enum | LegacyAdditive / FftOlaPilot / FftOlaFull |
| `enabled` | true | bool | Master enable/disable |

---

## B. Quality Modes

| Mode | FFT | Hop | Reprojection Iters | Time Resolution |
|------|-----|-----|-------------------|-----------------|
| Light | 512 | 128 | 1 | ~2.7 ms |
| Standard | 1024 | 128 | 2 | ~5.3 ms |
| HighFidelity | 1024 | 256 | 5 | ~5.3 ms |
| Ultra | 2048 | 128 | 3 | ~10.7 ms |
| Extreme | 4096 | 128 | 5 | ~21.3 ms |
| UltraExtreme | 8192 | 128 | 8 | ~42.7 ms |

---

## C. Style Presets (`StyleConfig` — 6-axis character system)

| Axis | Description |
|------|-------------|
| `warmth` | Even-harmonic saturation (tube-like) |
| `air_brightness` | HF extension slope (0 = darker, 1 = brighter) |
| `smoothness` | Upper-mid roughness suppression |
| `spatial_spread` | Stereo side recovery aggressiveness |
| `impact_gain` | Impact band (80–180 Hz) transient punch |
| `body` | Low-mid harmonic body reinforcement |

### Preset Values

| Preset | warmth | air | smooth | spatial | impact | body |
|--------|--------|-----|--------|---------|--------|------|
| Reference | 0.15 | 0.50 | 0.40 | 0.30 | 0.15 | 0.40 |
| Warm | 0.55 | 0.30 | 0.60 | 0.30 | 0.15 | 0.50 |
| BassPlus | 0.20 | 0.45 | 0.35 | 0.30 | 0.45 | 0.75 |
| Grand | 0.25 | 0.80 | 0.50 | 0.45 | 0.18 | 0.35 |
| Smooth | 0.20 | 0.30 | 0.75 | 0.25 | 0.12 | 0.50 |
| Vocal | 0.18 | 0.40 | 0.45 | 0.20 | 0.20 | 0.55 |
| Punch | 0.20 | 0.45 | 0.35 | 0.30 | 0.35 | 0.60 |
| Air | 0.10 | 0.90 | 0.35 | 0.40 | 0.10 | 0.30 |
| Night | 0.30 | 0.20 | 0.80 | 0.20 | 0.10 | 0.55 |

---

## D. Module Parameters

### M1: Damage Posterior Engine

| Parameter | Value | Description |
|-----------|-------|-------------|
| `update_interval` | 32 frames | Analysis period (~200–300 ms) |
| `EMA_ALPHA` | 0.3 | Smoothing factor |
| Clipping threshold | 0.99 | Absolute sample level |
| Clipping ratio scale | 10.0× | Clipped samples × 10, clamped to 1.0 |
| Compression crest ratio | `(12.0 - crest_dB) / 9.0` | 0–1 compression amount |
| Stereo degradation | <0.05 / <0.1 / <0.2 | Side/mid energy ratios for mono detection |
| Spectral slope range | -24.0 to 0.0 dB/oct | Clamped slope estimation range |
| Cutoff energy cliff | 10.0× | Energy ratio for cutoff detection |
| Cutoff frequency limit | 2000–20000 Hz | Valid detection range |

### M2: Tri-Lattice Analysis

| Lattice | FFT Size | Purpose |
|---------|----------|---------|
| Micro | 256 | Transient detail |
| Core | 1024 | Main synthesis |
| Air | 2048 | HF stability |

Window: Hann, normalization = 1/FFT_SIZE.

### M3: Structured Factorizer

**Spectral Flux (transient detection):**

| Parameter | Value | Description |
|-----------|-------|-------------|
| `flux_ema_alpha` | 0.05 | Slow EMA (~20 frames) |
| `flux_threshold_multiplier` | 3.0 | Transient when flux > 3× avg |
| `flux_baseline_constant` | 0.01 | Offset threshold |
| `PRE_ECHO_TRANSIENT_THRESHOLD` | 0.15 | Minimum activity for pre-echo |

**Harmonic Ridge Detection:**

| Parameter | Value | Description |
|-----------|-------|-------------|
| `f0_min` | 50 Hz | Min fundamental |
| `f0_max` | 4000 Hz | Max fundamental |
| `ridge_score_threshold` | 0.3 | Min score to accept f0 |
| `max_harmonics` (fill) | 32 | Harmonics to model |
| `max_harmonics` (score) | 16 | Harmonics in scoring |
| `tolerance` | 2 bins | Search window |
| `weight_formula` | 1/sqrt(n) | Decreasing with order |

**Air Field Extraction:**

| Parameter | Value | Description |
|-----------|-------|-------------|
| `band_width` | 20 bins | Width below cutoff for slope |
| `slope_range` | -24.0 to 0.0 dB/oct | Clamped |
| `default_slope` | -6.0 dB/oct | Fallback gentle rolloff |
| `max_freq` | 22000 Hz | Upper limit for air extrapolation |

### M4: Inverse Residual Solver

**Declipping:**

| Parameter | Value | Description |
|-----------|-------|-------------|
| `threshold` | 0.99 | Clipping detection |
| `peak_scale_conservative` | 0.15 | At transient=0.0 |
| `peak_scale_aggressive` | 0.20 | At transient=1.0 |
| `peak_max_ratio` | 1.5× | Clamped to threshold × 1.5 |
| `parabola_scale` | 4.0 | Cubic Hermite factor |

### M5: Self-Reprojection Validator

| Parameter | Value | Description |
|-----------|-------|-------------|
| `max_iters` | QualityMode-dependent (1–8) | Reprojection loops |
| `sample_scale` | 2/N | Additive synthesis normalization |
| `ola_hop_size` | 256 | OLA hop alignment |
| `fft_size` | 1024 (CORE) | OLA frame length |

### M6: Perceptual Safety Mixer

| Parameter | Value | Description |
|-----------|-------|-------------|
| `sigmoid_slope` | 4.0 | Crossover steepness |
| `true_peak_limit` | 1.0 | Hard clipper at 0 dBFS |
| K-weighted RMS EMA | ~300 ms | Loudness tracking window |

### M7: Quality Governor

| Parameter | Value | Description |
|-----------|-------|-------------|
| `avg_process_time_us_alpha` | 0.05 | CPU time EMA |
| `quality_downgrade_threshold` | 20000 us | Downgrade when > 20 ms |
| `quality_upgrade_threshold` | 5000 us | Upgrade when < 5 ms |

---

## E. APO-Specific (`apo_impl.rs`)

### Engine Lifecycle

| Parameter | Value | Description |
|-----------|-------|-------------|
| `PRIME_FRAMES` | 32 | Silent frames to settle OLA on lock |
| `CLICK_GATE_DELAY` | 6 frames | Delay before click gate |
| `CROSSFADE_FRAMES` | 32 | Fade duration for fresh engine (~352 ms) |
| `STARTUP_GUARD_FRAMES` | 8 | Zero-input protection window |

### DPC Latency Protection

| Parameter | Value | Description |
|-----------|-------|-------------|
| `DPC_SPIKE_RATIO_NORMAL` | 15.0× | Normal spike threshold |
| `DPC_SPIKE_RATIO_HIGH_LATENCY` | 50.0× | High-latency threshold |
| `DPC_SPIKE_RATIO_EXTREME` | 150.0× | Extreme threshold |
| `DPC_ADAPTIVE_SPIKE_THRESHOLD` | 3 | Spikes to trigger escalation |
| `DPC_ADAPTIVE_WINDOW` | 3000 frames | Spike counting window (~30 s) |
| `DPC_CALM_RECOVERY` | 6000 frames | Recovery window (~60 s) |
| `DPC_PROFILE_FRAMES` | 200 | Startup profiling (~2 s) |

### Frame Defaults

| Parameter | Value | Description |
|-----------|-------|-------------|
| `frame_size` | 480 samples | Windows Audio Engine (10 ms @ 48 kHz) |
| `sample_rate` | 48000 Hz | Default, negotiable |
| `channels` | 2 | Stereo |

---

## F. User Presets

### Default (all sliders at full, style axes at original defaults)

| Parameter | Value |
|-----------|-------|
| Strength | 100% |
| HF Reconstruction | 100% |
| Dynamics | 100% |
| Transient | 50% |
| Warmth | 15% |
| Air / Brightness | 50% |
| Smoothness | 40% |
| Body | 40% |
| Stereo Spread | 30% |
| Ambience Preserve | 0% |
| Impact Gain | 15% |

### Howard (warm, smooth, full spatial — designed for headphone listening)

| Parameter | Value |
|-----------|-------|
| Strength | 100% |
| HF Reconstruction | 100% |
| Dynamics | 100% |
| Transient | 100% |
| Warmth | 78% |
| Air / Brightness | 46% |
| Smoothness | 87% |
| Body | 100% |
| Stereo Spread | 100% |
| Ambience Preserve | 100% |
| Impact Gain | 100% |

Characteristics: Most params maxed. Air/Brightness lowered (reduces HF grain), Warmth high (vocal thickness), Smoothness high (suppresses upper-mid roughness). Full spatial + ambience for immersive headphone experience.

## G. Listener Feedback (2026-03-31)

Issues reported at original defaults (before Howard preset):

1. **HF over-reconstruction** — grainy, too stimulating → lowered `air_brightness` to 46%
2. **Vocal thickness loss** — mid body reduced → raised `body` to 100%, `warmth` to 78%
3. **Breath/non-breath gap** — transient continuity → raised `transient` to 100%, `smoothness` to 87%
4. **Mid-low spatial compression** — room tone suppressed → raised `ambience_preserve` to 100%
