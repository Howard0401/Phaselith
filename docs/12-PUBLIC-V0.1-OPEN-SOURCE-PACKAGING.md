# 12. Public v0.1 Open-Source Packaging

## Purpose

This document defines what the first public Phaselith repository release should look like.

The goal is not to dump an internal worktree onto GitHub.

The goal is to publish a repository that:

1. earns technical credibility
2. communicates the Phaselith architecture clearly
3. preserves a viable AGPL + commercial-license path
4. avoids leaking low-signal internal clutter
5. creates a strong first impression for stars, forks, and technical discussion

## Public v0.1 Principle

Public v0.1 should be:

1. technically real
2. architecturally impressive
3. commercially coherent
4. honest about current runtime maturity

It should not be:

1. a raw internal snapshot
2. a pile of temporary planning notes
3. a repo full of overclaimed marketing guarantees

## What Public v0.1 Should Include

The first public release should include:

1. the real browser runtime code
2. the real DSP core
3. the current Windows APO path, clearly labeled as transitional
4. the current documentation set
5. the AGPL license
6. the commercial-license notice
7. the contribution policy and CLA
8. the translated READMEs

In practical terms, the following areas are good public-facing core:

1. `README.md`
2. `README.zh-TW.md`
3. `README.zh-CN.md`
4. `docs/`
5. `chrome-ext/`
6. `crates/dsp-core/`
7. `crates/wasm-bridge/`
8. `crates/apo-dll/`
9. `crates/tauri-app/`

## What Public v0.1 Should Exclude

The first public release should not include:

1. generated zip artifacts
2. internal planning drafts that were never meant to be canonical docs
3. local environment or assistant-control files
4. binary clutter that is not necessary for understanding or building the project

As of the current repo state, the following items should stay out of the first public commit:

1. `phaselith-extension.zip`
2. `PHASELITH_Architecture_Replan_Review.md`
3. `PHASELITH_FFT_Full_Implementation_Plan.md`
4. `PHASELITH_Safe_Replan.md`
5. `.claude/`

For local distribution packaging, extension store copy such as `chrome-ext/STORE-LISTING.md`
should also stay out of the shipped zip artifact.

## Required Pre-Public Checks

Before making the repository public:

1. ensure `README.md` reflects the real current architecture
2. ensure store copy does not overclaim impossible guarantees
3. ensure licensing documents consistently identify the Project Owner
4. ensure no secrets, private keys, or environment-specific credentials are tracked
5. ensure ignored generated artifacts are actually excluded
6. ensure the browser runtime is described honestly as the current listening reference
7. ensure the APO runtime is described honestly as transitional

## Messaging Rules for Public v0.1

Public v0.1 should emphasize:

1. real-time perceptual restoration and playback optimization
2. masking reduction
3. center image and front-focused presentation
4. explainable architecture
5. browser and desktop runtime ambition

Public v0.1 should avoid saying:

1. guaranteed lossless recovery
2. guaranteed perfect restoration
3. impossible latency guarantees without context
4. universal claims such as "never makes audio worse"

## Recommended Repo Narrative

The first public impression should be:

1. this is a serious DSP project
2. it has a coherent architecture
3. it already works
4. it is ambitious but honest about limitations
5. it is open-source, but commercially intentional

The repo should feel like:

1. a flagship technical project
2. a real product-in-progress
3. a credible open-source launch

It should not feel like:

1. a private scratchpad
2. a marketing-only landing page
3. a chaos dump of every internal note

## Current Public-Readiness Status

At the current stage:

1. `README` is already strong enough to serve as a public landing page
2. `docs/` is already structured enough to support serious technical reading
3. licensing and contribution documents are already viable for AGPL + commercial dual use
4. the store listing copy has been reduced to more defensible claims
5. obvious generated/internal artifacts have been moved into `.gitignore`

The biggest remaining strategic choice is whether to publish:

1. `docs/10-DEFENSIVE-PUBLICATION.md`

That file is useful if the goal is prior-art value and idea-space staking.

It is less useful if the goal is to keep the method space narrower for now.

## Decision on Defensive Publication

The public v0.1 launch can follow one of two valid routes:

### Route A: Public + Defensive

Keep `docs/10-DEFENSIVE-PUBLICATION.md` in the public repository.

Use this route when the priority is:

1. prior-art style disclosure
2. technical staking of the Phaselith method space
3. maximum architectural visibility

### Route B: Public + Narrower Disclosure

Keep the repository public, but temporarily remove or hold back `docs/10-DEFENSIVE-PUBLICATION.md`.

Use this route when the priority is:

1. public reputation
2. code credibility
3. AGPL distribution
4. more cautious disclosure of the broader idea space

## Recommended Default

If the public goal is:

1. GitHub reputation
2. strong project identity
3. technically impressive open-source packaging

then public v0.1 should proceed with:

1. the current codebase
2. the current documentation set
3. the AGPL + commercial-license structure
4. internal planning drafts excluded
5. generated artifacts excluded

The only item that should remain a deliberate decision is:

1. whether `docs/10-DEFENSIVE-PUBLICATION.md` ships on day one

## Public v0.1 Release Checklist

Use this checklist before switching the repository to public:

1. README reviewed
2. store-facing wording reviewed
3. licensing wording reviewed
4. Project Owner named explicitly
5. internal planning drafts excluded
6. generated zip excluded
7. `.claude/` excluded
8. docs index updated
9. public-first commit reviewed
10. defensive-publication decision made
