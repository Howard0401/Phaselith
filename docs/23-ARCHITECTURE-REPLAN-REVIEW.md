# Phaselith Architecture Replan Review

Version: Draft v1
Scope: review of the current architecture and proposed replanning, assuming browser may become the primary flagship product route.

## 1. Executive Conclusion

Browser can absolutely become the **primary commercial flagship**.

However, in the current codebase, browser is **not yet a true flagship engine path**.
It is currently:
- commercially promising
- sonically impressive
- strong in character and image behavior

but architecturally it is still a:
- CPU-only
- dual-mono
- worklet-constrained
- partially placeholder-validated

path.

Therefore the correct replanning is:

`Browser may be the flagship product, but it must not be confused with the current browser implementation.`

## 2. High-Priority Review Findings

### [P1] Browser path is structurally dual-mono, so it cannot yet be the true flagship stereo engine

Current WASM bridge builds two separate mono engines and processes left/right independently:
- `with_channels(1)` in the bridge
- `process_block_ch(0/1)` per channel

References:
- `crates/wasm-bridge/src/lib.rs`
- `chrome-ext/src/worklet-processor.js`

Implication:
- browser currently preserves and enhances center/focus well
- but it does not yet run a truly stereo-aware flagship path
- any spatial recovery or stereo-field intelligence is structurally limited

This is the single biggest architectural reason browser is not yet a "full flagship" engine.

### [P1] The validation path is still partially placeholder, so the "flagship restoration" claim is ahead of the implementation

M5 still writes validated output using a placeholder frequency-bin-to-time approximation rather than a proper ISTFT/OLA reconstruction.

Reference:
- `crates/dsp-core/src/modules/m5_reprojection/mod.rs`

Implication:
- current result can sound very strong
- but the core restoration path is not yet a mathematically mature flagship implementation
- the current wow factor comes partly from character + focus behavior, not only from full residual restoration

### [P1] `combine_residuals()` still omits transient and side residuals from the final validation candidate

Current M5 combination includes:
- harmonic
- air
- weak phase contribution

but does not include:
- transient residual
- side residual

Reference:
- `crates/dsp-core/src/modules/m5_reprojection/mod.rs`

Implication:
- browser cannot yet fully cash in on punch and stereo reconstruction
- current sound is biased toward openness, smoothness, center cleanup, and premium presentation

This is one reason the result can feel luxurious but not fully "complete" as a flagship reconstruction engine.

### [P1] APO path is not currently taking advantage of native flagship conditions

APO creates a multi-channel engine, but then processes multi-channel audio by:
- de-interleaving each channel
- allocating a `Vec` inside `process()`
- processing channels separately
- resetting engine state between channels

Reference:
- `crates/apo-dll/src/apo_impl.rs`

Implication:
- the native path is currently under-architected
- browser sounding better than expected is not surprising
- APO is not yet the real reference implementation it should be

### [P2] The current architecture mixes two product identities inside one runtime path

Current config and M6 now include a style / character layer that intentionally ensures an audible effect even on pristine sources.

References:
- `crates/dsp-core/src/config.rs`
- `crates/dsp-core/src/modules/m6_mixer/mod.rs`

Implication:
- current browser sound is strong because it behaves partly like restoration and partly like a premium character enhancer
- this is commercially powerful
- but it blurs the line between "flagship restoration" and "flagship sound personality"

This must be made explicit in the new architecture.

## 3. What the Current Architecture Is Actually Good At

The current browser path is already very strong at:
- front-facing center image
- reduced roughness
- premium presentation
- cheap-headphone uplift
- vocal focus
- luxurious sound personality

This is not a failure.
It means the product has already discovered its strongest commercially valuable behavior.

The current architecture is weaker at:
- full stereo-aware restoration
- validated time-domain reconstruction
- native-path superiority
- mathematically mature flagship residual synthesis

## 4. Replanning Principle

Do not ask:

`Should browser be flagship or not?`

Ask:

`What kind of flagship should browser be?`

Recommended answer:

`Browser should become the flagship consumer product, while native/APO should become the flagship reference engine path.`

This is not a contradiction.

Meaning:
- browser is the main product most users buy first
- APO/native is the most structurally complete and technically pure runtime

## 5. New Three-Layer Architecture

## 5.1 Layer A: Restoration Core

Purpose:
- damage-driven repair
- source-conditioned residual recovery
- confidence-gated validation

Must contain:
- M1 damage posterior
- M2 tri-lattice analysis
- M3 factorizer
- M4 residual solver
- M5 reprojection validation

This layer should be the same conceptual engine across hosts, but not necessarily identical in fidelity.

## 5.2 Layer B: Character Layer

Purpose:
- premium-system presentation
- style identity
- stable audible uplift even on mixed-quality sources

Should own:
- warmth
- smoothness
- body
- air brightness
- spatial spread
- impact gain

This layer explains why browser already sounds strong.

## 5.3 Layer C: Host Runtime Adapter

Purpose:
- host-specific constraints
- block size
- stereo semantics
- latency budget
- memory model

Hosts:
- Browser AudioWorklet
- APO native runtime
- future desktop or plugin runtime

Important:
- host adapters must not define product identity
- they only define how much of the core can safely run

## 6. Recommended Product Topology

## 6.1 Browser = flagship product route

Browser should be:
- the main user acquisition route
- the flagship consumer experience
- the style-rich route
- the one with the strongest immediate wow factor

But browser should **not** be required to be the mathematically deepest engine.

Product promise:
- strongest perceived upgrade
- easiest install
- richest style personality
- best first impression

## 6.2 APO = flagship reference runtime

APO should be:
- the canonical stereo-aware engine
- the runtime where the most correct center image logic lives
- the host that fully exploits interleaved multi-channel processing

Product promise:
- best structural correctness
- best host integration
- best path for future reference / desktop / audiophile mode

## 7. Concrete Replan

## 7.1 Browser path

Browser should be replanned as:

`Phaselith Browser Signature Engine`

Rules:
- keep CPU-only for now
- keep AudioWorklet-safe
- keep no-blocking, no worker dependency on hot path
- keep strong character layer
- preserve front-image magic

But change:
- move away from permanent dual-mono as the end state
- introduce a stereo-aware browser core when safe
- if full stereo engine is too expensive, at least add a shared cross-channel analysis context outside the per-channel solver

Meaning:
- browser can remain the flagship product
- but should evolve from "two mono engines" into "stereo-informed browser engine"

## 7.2 APO path

APO should be replanned as:

`Phaselith Native Reference Engine`

Rules:
- true interleaved multi-channel processing
- no allocation in `process()`
- no per-channel `Vec`
- no reset between left/right channel processing
- canonical stereo-aware restoration path

Meaning:
- APO becomes the real reference path
- browser remains the best product surface

## 7.3 M5/M6 split

Current architecture lets M6 carry too much of the product magic.

Replan:
- M5 should become a stronger true reconstruction/validation stage
- M6 should remain the safety and character integration layer
- but M6 should no longer compensate for missing structural depth in M5

This is the most important engine maturity step.

## 8. New Definition of "Flagship"

To avoid confusion, define two different flagship concepts.

### Product flagship

The route users buy and recommend first.

Recommended:
- Browser

### Engine flagship

The most structurally complete, stereo-correct, technically mature runtime.

Recommended:
- APO / native

This solves the current tension.

## 9. What Should Not Be Done

Do not do these:
- do not force browser to pretend it is the most mathematically complete engine right now
- do not kill the current character layer just to sound more "serious"
- do not make APO secondary forever
- do not keep dual-mono browser semantics as the permanent endgame
- do not call the current implementation "full flagship restoration"

## 10. Immediate Priorities

### Priority 1

Stabilize and protect the current front-image / premium magic.

### Priority 2

Refactor host roles:
- Browser = flagship consumer route
- APO = flagship reference runtime

### Priority 3

Upgrade M5 from placeholder validation toward real reconstruction.

### Priority 4

Make stereo semantics explicit:
- browser stereo-informed
- APO stereo-native

### Priority 5

Keep style system, but explicitly brand it as a first-class product layer.

## 11. Final Recommendation

If you personally feel browser should be the flagship route, that instinct is commercially sound.

But the correct architectural version of that idea is:

`Browser should be the flagship experience, not the only flagship definition.`

And the replan should be:

- Browser = `Flagship Signature`
- APO = `Flagship Reference`

That gives you:
- commercial clarity
- technical honesty
- room to grow
- no need to throw away what already sounds strong
