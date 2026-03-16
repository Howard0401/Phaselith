# Build Profiles and Real-Time Audio Safety

## Problem

The Phaselith APO DLL runs inside `audiodg.exe` on a **real-time audio thread**.
A debug build (`opt-level = 0`) is 5–10× slower than release, causing CPU overruns
that manifest as **periodic crackling** — the audio thread cannot finish processing
a 528-sample block (11 ms budget at 48 kHz) before the next block arrives.

### How We Discovered This

| Build | DLL size | UltraExtreme max block time | Result |
|-------|----------|-----------------------------|--------|
| Debug (`opt-level = 0`) | 12.2 MB | >11 ms (estimated) | ❌ Periodic crackling |
| Dev + per-crate opt (`opt-level = 3`) | 2.0 MB | ~4.2 ms | ✅ Clean |
| Release (`opt-level = 3` + LTO) | 1.2 MB | ~4.2 ms | ✅ Clean |

The `cargo tauri dev` command builds everything in debug mode. When the user
clicked "Install" in the Tauri UI, the **12 MB debug DLL** was deployed to
`C:\Program Files\Phaselith\`. The UltraExtreme quality mode (FFT 8192,
8 reprojection iterations, hop 2048) requires substantial computation;
without compiler optimizations the DSP loop exceeded the real-time deadline
on every processing hop (~every 4th block), producing audible clicks at
~23 Hz (once per hop cycle).

## Solution: Per-Crate Dev Profile Override

In the workspace `Cargo.toml`:

```toml
# ─── Real-time audio safety ───
# APO DLL runs in audiodg.exe's real-time thread.
# Debug build is 5-10x slower → CPU overruns → crackling.
# Always compile the APO and DSP core with optimizations,
# even during `cargo tauri dev` (debug mode).
[profile.dev.package.phaselith-apo]
opt-level = 3

[profile.dev.package.phaselith-dsp-core]
opt-level = 3
```

This ensures the **DSP core and APO crate** are always compiled with full
optimizations, even during development. The Tauri UI crate remains at
`opt-level = 0` for fast iteration and debuggability.

### Why This Works

Cargo supports [profile overrides](https://doc.rust-lang.org/cargo/reference/profiles.html#overrides)
at the per-package level. `profile.dev.package.<name>` applies **only** when
building with the `dev` profile (the default for `cargo build` and
`cargo tauri dev`). Release builds are unaffected.

## Industry Practice

Professional audio companies (iZotope, FabFilter, Waves, etc.) follow
the same principle:

1. **DSP core — always optimized.** Real-time audio code is never tested
   without compiler optimizations. Even "debug" builds use `-O2` or higher
   for the signal processing layer.

2. **UI layer — debug-friendly.** The control surface / editor can remain
   unoptimized for faster compile times and easier debugging.

3. **RelWithDebInfo.** The standard practice is "Release with Debug Info" —
   full optimization plus debug symbols. Cargo's `opt-level = 3` with
   `debug = true` (the dev profile default) achieves exactly this for the
   overridden crates.

4. **CI performance gates.** Every commit runs a timing benchmark
   (e.g., `apo_ultraextreme_528_block`) to catch regressions before they
   ship. If max block time exceeds budget, the build fails.

## Rules

1. **Never install a debug-built APO DLL** for listening or testing.
   The per-crate override prevents this by default.

2. **Never remove the `profile.dev.package` overrides** from `Cargo.toml`
   without understanding the real-time implications.

3. **If adding a new crate that runs on the audio thread**, add a
   corresponding `[profile.dev.package.<crate>]` override with
   `opt-level = 3`.

4. **Test with the actual APO quality mode** (`UltraExtreme`), not just
   `Standard`. The `apo_ultraextreme_528_block` test exists for this.

## Related

- [05-WINDOWS-APO-RUNTIME-FLOW.md](05-WINDOWS-APO-RUNTIME-FLOW.md) — APO lifecycle and real-time constraints
- [14-M2-STFT-ENGINE-CRACKLING.md](14-M2-STFT-ENGINE-CRACKLING.md) — Previous crackling root cause (different issue)
- [15-APO-CHROME-COEXISTENCE.md](15-APO-CHROME-COEXISTENCE.md) — APO + Chrome conflict
