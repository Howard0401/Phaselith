# SFX → EFX: Fixing Multi-Stream Playback Distortion

**Date**: 2026-03-18
**Version**: v0.1.22
**Scope**: APO registration, Tauri install script, COM binding

---

## Symptom

With APO installed, playing audio from two or more sources simultaneously (e.g. two browser tabs, game + Discord) produced audible crackling/distortion. Stopping one source or uninstalling the APO eliminated the issue.

Earlier investigation directions (CPU overload, engine still processing in disabled state) were dead ends. The actual root cause was a **race condition**.

## Root Cause

The APO was registered as **SFX (Stream Effect)**, meaning Windows created a separate APO instance for each audio stream:

```
YouTube tab  ──→ [APO instance A] ──┐
                                    ├──→ mixer ──→ DAC
Bilibili tab ──→ [APO instance B] ──┘
```

All instances shared the same DLL address space and these global statics:

```rust
static STORED_ENGINES: Mutex<Option<StoredEngines>>   // engine handoff
static STORED_PRIME: Mutex<Option<StoredPrimeAudio>>   // written every process()
static STORED_OUTPUT: Mutex<Option<StoredOutputBlock>>  // crossfade data
static LAST_OUTPUT_L_BITS: AtomicU32                    // both instances write
static LAST_OUTPUT_R_BITS: AtomicU32
```

These statics were designed for **sequential instance recycling** within a single stream (e.g. Windows unlock → lock during YY Voice join/leave). Under SFX, multiple stream instances **coexist and run concurrently**, causing:

1. **Engine theft**: Instance A stores engines on unlock → Instance B overwrites on unlock → A restarts and takes B's engine (wrong OLA state)
2. **Prime data corruption**: Both instances interleave writes to the same ring buffer → new instance primes with mixed audio from different streams
3. **Last output clobbering**: Click gate seed value gets overwritten by another stream's output

## Fix: Switch to EFX (Endpoint Effect)

```
YouTube tab  ──┐
               ├──→ mixer ──→ [APO instance (single)] ──→ DAC
Bilibili tab ──┘
```

EFX sits after the mixer and before the DAC — only **one instance** exists per endpoint. Regardless of how many streams are playing, the APO sees only the final mixed signal.

### Registry Key Mapping

| Purpose | Property ID | Old (SFX) | New (EFX) |
|---------|------------|-----------|-----------|
| CompositeFX | `{d04e05a6...}` | `,13` | `,15` |
| V1 (single CLSID) | `{d04e05a6...}` | `,5` | `,7` |

## Changes

### 1. `crates/apo-dll/src/registry.rs`
- `PKEY_FX_ENDPOINT_EFFECT_CLSID_NAME` = `,15` (new EFX key)
- `MaxInstances` from `0xFFFFFFFF` → `1`
- bind/unbind cleans up legacy SFX keys

### 2. `crates/tauri-app/src/com_bind.rs`
This is the actual path that writes to the registry via COM `IPropertyStore`.
- Added `PKEY_COMPOSITEFX_EFX` (pid=15) and `PKEY_FX_EFX_V1` (pid=7)
- On bind: remove all legacy SFX keys (`,5`, `,13`) first, then write EFX key
- On unbind: remove all EFX + SFX keys
- Added `remove_from_property()` helper

### 3. `crates/tauri-app/src/install_script.ps1`
Elevated PowerShell install script (handles TrustedInstaller permissions).
- Added `$pkeyCompositeEfx` (`,15`) and `$pkeyV1Efx` (`,7`)
- Step 4a: Clean up legacy SFX registrations
- Step 4b: Try CompositeFX EFX (`,15`)
- Step 4c: Fallback to V1 EFX (`,7`)

### 4. `crates/apo-dll/src/apo_impl.rs` + `apo_com.rs`
- Updated comments: SFX → EFX
- Added SAFETY comments on global statics (EFX guarantees single instance, no race)

## Pitfall: registry.rs Is Not the Actual Install Path

First attempt only changed `registry.rs` — had zero effect. The `bind_to_all_render_endpoints()` in `registry.rs` is a fallback path in the APO DLL itself. **The actual install uses `com_bind.rs` and `install_script.ps1` from the Tauri app.**

Running `reg query` revealed our CLSID was at `,5` (V1 SFX), not at `,13` (CompositeFX SFX) or `,15` (CompositeFX EFX) — this was written by `com_bind.rs`'s V1 fallback.

**Lesson**: When changing registry keys, find **all write paths**, not just one.

## EFX vs SFX Audio Quality

In theory, SFX sees the raw clipped signal from a single app, while EFX sees the mixed signal (clipping features diluted by other streams). In practice:
- 99% of the time only one primary source is playing
- System notification sounds / Discord calls are low volume, negligible dilution
- M1 damage detection is per-frame adaptive, handles mixed signals fine

## Verification

Check `apo_debug.log` and confirm:
- Only **1** `LockForProcess` → `lock_for_process done`
- Only **1** `APOProcess: first call!`
- Starting a second audio source does **not** produce a new `LockForProcess`
