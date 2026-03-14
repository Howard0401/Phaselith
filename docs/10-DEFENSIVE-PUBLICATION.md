# 10. Defensive Publication

## Purpose

This document is a defensive-publication-style technical disclosure for CIRRUS.

Its purpose is:

1. to publicly describe the core technical ideas behind CIRRUS in a structured way
2. to create a clearer prior-art style record than a normal README or architecture note
3. to distinguish the CIRRUS approach from conventional EQ, virtualizer, codec-side enhancement, and offline restoration workflows

This document is not legal advice.

It is a technical disclosure intended to make the current design space more explicit and easier to timestamp, cite, and reason about.

## Technical Field

The disclosed subject matter relates to:

1. real-time digital audio restoration
2. perceptual audio enhancement
3. low-latency playback correction
4. browser-hosted, system-wide, or host-integrated DSP pipelines

More specifically, it concerns a system that:

1. estimates degradation or collapse in incoming audio
2. generates repair candidates in multiple signal domains
3. validates those candidates against a degradation-consistency model
4. mixes only accepted repairs under perceptual and safety constraints

## Problem Statement

Everyday playback often suffers from one or more of the following:

1. lossy-codec haze
2. high-frequency cutoff or under-expression
3. clipping and limiting damage
4. spatial collapse or weak center image
5. transient smear
6. harshness and codec-like roughness
7. poor source placement on headphones, laptop speakers, or low-end playback chains

Conventional solutions usually do one of four things:

1. apply fixed EQ curves
2. apply widening, virtualizing, or room-like effects
3. apply loudness, compression, or tone shaping
4. run offline repair tools with high latency and human supervision

These approaches do not adequately solve the following combined requirement:

1. real-time operation
2. source-adaptive behavior
3. low-latency host compatibility
4. protection against over-repair
5. preservation of a stable center image and premium-system presentation

## High-Level Disclosure

CIRRUS discloses a staged method for real-time perceptual audio restoration and presentation in which:

1. a degradation posterior is estimated from the incoming signal
2. the signal is decomposed into structured fields
3. repair candidates are generated in frequency and time domains
4. candidate repairs are self-validated by reprojection through a degradation model
5. validated repairs are mixed through a perceptual safety layer

Unlike a fixed equalizer, the disclosed method does not start by choosing a tonal curve.

It starts by estimating:

1. what kind of damage is present
2. where perceptually useful structure is likely suppressed
3. which repairs survive a consistency check

## Distinction from Conventional EQ

The disclosed method differs from a conventional EQ in at least the following ways:

1. it estimates damage instead of applying a fixed boost/cut curve
2. it creates candidate residual repairs instead of directly changing a transfer function
3. it validates repairs against a degradation-consistency criterion
4. it separately reasons about harmonic, air, transient, spatial, and time-domain components
5. it uses safety gating and perceptual mixing instead of unconditional frequency shaping

Perceived tonal change may occur as a side effect, but tonal change is not the principal mechanism.

## Distinction from Conventional Virtualization or Spatial Effects

The disclosed method is also distinct from classic spatial processing because it does not principally rely on:

1. fixed crossfeed
2. synthetic HRTF-based virtual speaker rendering
3. delay-line widening
4. artificial room simulation

Instead, the disclosed method can increase front focus or center lock by reducing masking, structural haze, and degradation-related instability in shared or center-bearing components.

## Distinction from Offline Restoration

The disclosed method is distinct from offline restoration workflows because it is designed for:

1. bounded latency
2. continuous host callbacks
3. system-wide or browser-hosted playback
4. always-on use without per-track manual editing

Accordingly, the disclosed method prefers:

1. staged heuristics
2. constrained residual solving
3. bounded validation
4. low-overhead synthesis and mixing

over large, unconstrained, offline optimization.

## Core Processing Stages

The current CIRRUS architecture is organized as M0-M7.

### M0. Orchestrator

The orchestrator:

1. receives host blocks
2. accumulates frame and hop state
3. exposes aligned analysis views
4. publishes dry signal context for later stages

This stage may use ring buffers, frame clocks, and hop scheduling.

### M1. Damage Posterior

The damage posterior estimates one or more of:

1. cutoff likelihood
2. clipping severity
3. limiting or compression severity
4. stereo collapse likelihood
5. overall confidence

The disclosed method does not require only one damage type.

It may maintain a multi-factor posterior over simultaneous damage modes.

### M2. Multi-View Analysis Lattice

The signal is transformed into one or more time-frequency views, including:

1. a core lattice
2. one or more short-window views for transient sensitivity
3. one or more long-window views for smoother spectral or air estimation

These views can share FFT infrastructure while preserving different effective time-frequency tradeoffs.

### M3. Structured Factorization

The lattice is factorized into fields such as:

1. harmonic field
2. air field
3. transient field
4. spatial field

The factorization need not be fully source-separated.

It is sufficient that the fields provide useful structure for later candidate generation.

### M4. Inverse Residual Solver

One or more candidate repairs are generated, including:

1. harmonic extension candidates
2. air continuation candidates
3. transient or declipping time-domain candidates
4. side or spatial recovery candidates
5. phase relaxation candidates

The disclosed method allows these candidates to exist in different domains.

For example:

1. harmonic and air candidates may be frequency-domain residuals
2. declipping candidates may be time-domain residuals
3. spatial candidates may be per-band or per-field bias terms

### M5. Self-Reprojection Validation

This is one of the most important disclosed elements.

A candidate repair is not automatically trusted.

Instead, a repaired signal or repaired representation is tested against a degradation-consistency process.

Conceptually:

1. start from original signal `x`
2. generate candidate repair `r`
3. form repaired signal `x + r`
4. pass `x + r` through a degradation model `D`
5. measure whether `D(x + r)` remains acceptably consistent with `x`

If the candidate fails consistency requirements:

1. shrink it
2. gate it
3. reject it

This differs from blind enhancement systems that only predict a desired output.

In the disclosed method, a repair is preferred when it remains stable under re-degradation.

### M6. Perceptual Safety Mixer

Validated repairs are mixed with the dry signal through a safety and presentation stage that may include:

1. headroom-aware residual injection
2. low-band protection
3. loudness compensation
4. optional character shaping
5. controlled ambience preservation
6. transient-aware or masking-aware guards

This stage can preserve the user-facing requirement that the system remain listenable and system-friendly even in always-on playback use.

### M7. Governor

The system may publish runtime state for:

1. control surfaces
2. debugging
3. telemetry
4. confidence display
5. host adaptation

## Key Technical Ideas

The following ideas are central to the disclosed method.

### A. Damage-Aware Repair Instead of Fixed Processing

The system first estimates what type of damage is present.

This allows different repairs to respond differently to:

1. cutoff
2. clipping
3. collapse
4. compression-like flattening

### B. Residual-Based Repair Instead of Static Tone Curves

Rather than directly prescribing a target EQ curve, the system constructs residual additions:

1. what can be safely added back
2. what should remain untouched
3. what should be partially restored

### C. Validation by Reprojection

The self-reprojection step allows the system to reject repairs that look attractive but are not robust under the assumed damage model.

This creates a technical distinction from:

1. static enhancers
2. unconditional exciters
3. unvalidated bandwidth extension

### D. Domain Separation

The disclosed method may maintain separate repair channels for:

1. frequency-domain residuals
2. time-domain residuals
3. spatial or consistency control values

This reduces the risk of mixing conceptually different signals into a single untyped path.

### E. Perceptual Safety and Presentation Control

The system may deliberately include a bounded presentation layer after validation, provided that:

1. it is constrained
2. it does not dominate the restoration logic
3. it preserves stable center image and listenability

## Example Processing Sequence

An example implementation sequence may be:

1. receive a host block
2. update frame and hop state
3. estimate damage posterior
4. generate one or more time-frequency views
5. estimate harmonic, air, transient, and spatial fields
6. generate one or more residual candidates
7. validate candidate residuals via reprojection
8. mix validated residuals with dry signal under loudness and safety constraints
9. output processed audio and telemetry

## Example Candidate Types

Examples of candidate repair types include:

1. extension above a detected cutoff using harmonic structure below that cutoff
2. air continuation with decay or tilt control
3. clipped-sample back-projection in time domain
4. side or collapse recovery conditioned on spatial confidence
5. phase or coherence relaxation in upper bands

## Example Validation Metrics

The disclosed method may use one or more of:

1. reconstruction error
2. consistency error after re-degradation
3. perceptually weighted error
4. masking-aware error
5. soft acceptance or Wiener-style gain masks
6. transient-aware constraints

## Example Safety and Presentation Constraints

The disclosed method may apply one or more of:

1. peak or true-peak protection
2. loudness compensation
3. masking threshold guards
4. transient flags
5. controlled ambience-preserve gates
6. limited character shaping

## Emergent Technical Effects

A system built according to the disclosed method may create one or more of the following observable results:

1. reduced muddiness without classic EQ coloration
2. stronger center image or front-focused source placement
3. reduced haze on poor playback chains
4. improved intelligibility on laptop speakers or low-cost transducers
5. reduced apparent harshness while maintaining high detail

These outcomes may be large in perception even when overall waveform correlation remains high, because human hearing is highly sensitive to:

1. masking release
2. direct-versus-diffuse contrast
3. center-bearing shared components
4. transient clarity
5. source-localization cues

## Why Small Waveform Differences Can Produce Large Audible Differences

The disclosed system may create large audible changes even when gross waveform difference appears numerically small.

Reasons include:

1. the change may be concentrated in perceptually critical bands rather than spread uniformly
2. center-bearing or shared components can dominate source localization
3. masking reduction can expose pre-existing information without requiring a large energy change
4. transient and direct-sound structure can strongly influence presentation despite limited sample-wise deviation

Accordingly, low overall waveform deviation does not imply a small perceptual effect.

## Example Implementation Variants

The disclosed approach includes, but is not limited to, the following variants:

1. browser-hosted AudioWorklet implementation
2. system-wide desktop runtime such as Windows APO
3. future Core Audio or PipeWire runtimes
4. additive synthesis back to time domain
5. FFT/OLA-based synthesis back to time domain
6. transient-aware switching or blending between synthesis paths
7. masking-aware validation
8. ambience-preserve gates that restore only diffuse-tail-like components

## Non-Limiting Boundaries of the Disclosure

This disclosure does not require:

1. machine learning
2. training data
3. a single synthesis path
4. a single error metric
5. a single host platform

The inventive space includes the general pattern of:

1. damage-aware estimation
2. structured residual generation
3. reprojection-based validation
4. constrained perceptual mixing

## Practical Prior-Art Intent

This document is intended to publicly disclose the following broad concept space:

1. real-time audio repair by residual generation plus self-reprojection validation
2. multi-field structured factorization for playback restoration
3. domain-separated residual handling for low-latency audio repair
4. perceptual safety mixing of validated repairs for always-on playback

The document is intentionally broader than a normal implementation note so that the disclosed idea space is not limited only to one exact source tree revision.

## Related Documents

For repository-specific detail, see:

1. [03-CORE-ALGORITHM.md](03-CORE-ALGORITHM.md)
2. [04-BROWSER-RUNTIME-FLOW.md](04-BROWSER-RUNTIME-FLOW.md)
3. [05-WINDOWS-APO-RUNTIME-FLOW.md](05-WINDOWS-APO-RUNTIME-FLOW.md)
4. [08-ROADMAP-AND-LIMITATIONS.md](08-ROADMAP-AND-LIMITATIONS.md)
5. [09-LICENSING-AND-CONTRIBUTIONS.md](09-LICENSING-AND-CONTRIBUTIONS.md)
