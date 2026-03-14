# 03. Core Algorithm

## Core Idea

CIRRUS does not start from "boost this band" or "widen that channel."

It starts from a different question:

What structure in the signal is likely damaged, masked, collapsed, or perceptually under-expressed, and what repairs can be added back without making the result fake?

That leads to a staged system:

1. estimate damage
2. decompose signal structure
3. generate repair candidates
4. validate those candidates against a degradation model
5. mix only what survives perceptual and safety checks

## Why CIRRUS Is Not a Traditional EQ

EQ changes a transfer curve.

CIRRUS changes the relationship between:

1. direct structure and diffuse haze
2. harmonic content and masking
3. transient edges and smear
4. center image and collapse
5. residual repairs and safety constraints

It may change the perceived tonal balance as a side effect, but tonal shift is not the primary mechanism.

## Module Overview

### M0. Orchestrator

Purpose:

1. collect host blocks
2. align frame and hop timing
3. expose stable analysis windows

### M1. Damage Posterior

Purpose:

1. estimate cutoff
2. estimate clipping and limiting
3. estimate stereo collapse
4. track overall confidence

### M2. Tri-Lattice

Purpose:

1. build analysis lattices
2. preserve multiple time-frequency views
3. support harmonic, air, and transient reasoning

### M3. Factorizer

Purpose:

1. estimate harmonic field
2. estimate air field
3. estimate transient behavior
4. estimate spatial field

### M4. Inverse Residual Solver

Purpose:

1. construct residual candidates from the structured fields
2. create both frequency-domain and time-domain candidates
3. estimate side recovery, harmonic extension, air continuation, and phase relaxation

### M5. Self-Reprojection Validator

Purpose:

1. send candidate repairs through a degradation-consistency check
2. shrink or reject repairs that do not survive reprojection
3. synthesize validated residuals back toward time domain

### M6. Perceptual Safety Mixer

Purpose:

1. mix validated residuals with the dry signal
2. protect peaks and long-term listenability
3. apply loudness compensation
4. manage character shaping
5. preserve a controlled amount of ambience

### M7. Governor

Purpose:

1. publish telemetry
2. expose runtime state
3. support control and diagnostics

## Emergent Listening Behavior

The system can produce effects that listeners often describe as:

1. less muddy without sounding EQ'd
2. more front-focused without obvious widening tricks
3. cleaner center image
4. more premium or "high-end" presentation
5. reduced haze on poor playback chains

The current working hypothesis is that this often happens because CIRRUS reduces spectral, temporal, and spatial masking while tightening the relationship between direct structure and the listener's source-localization cues.

## Guardrails

Any future change must be judged against these questions:

1. Does the center image get stronger or weaker?
2. Does the source move forward or collapse backward?
3. Does clarity improve because masking drops, or only because tone got brighter?
4. Is ambience preserved where it matters, or did the engine become too dry?
5. Does the result survive long listening, not just first-impression wow?
