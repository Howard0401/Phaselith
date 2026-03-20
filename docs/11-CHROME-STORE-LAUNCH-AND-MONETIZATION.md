# 11. Chrome Store Launch and Monetization

## Purpose

This document defines how Phaselith should be introduced on the Chrome Web Store, how Early Access transitions to paid, and the pricing model.

## Strategy

Use **one Chrome Web Store listing** throughout all phases.

### Phase 1 — Early Access (Current)

- Public Early Access
- Free tier: Strength capped at 50%, Reference preset only
- Pro tier: Full strength (0-100%), all 7 presets, full HF/dynamics control
- Pricing: **$4.99/month or $39.99/year**

### Phase 2 — Mature Product

- Same listing, accumulated reviews and installs
- Pricing adjustments based on market feedback
- Desktop APO remains a separate higher-tier product line

## Pricing Model

### Chrome Extension

| Tier | Price | Features |
|------|-------|----------|
| Free | $0 | Strength ≤50%, Reference preset only, all audio sources |
| Pro | $4.99/month or $39.99/year | Full strength, all 7 presets, full controls |

### Desktop APO (separate product)

| Tier | Price | Features |
|------|-------|----------|
| TBD | TBD | Full system-wide audio restoration |

## Product Positioning

**Marketing tagline:**

> ☕ One coffee a month — turn your YouTube and Spotify into studio-quality audio through your existing headphones.

Phaselith should be described as:

1. a real-time audio restoration engine (not an EQ or bass booster)
2. a way to get DAC-quality improvement without buying new hardware
3. a self-validating system that only applies mathematically verified repairs
4. affordable premium audio for everyone

Phaselith should NOT be described as:

1. a simple EQ
2. a bass booster
3. a loudness enhancer
4. a fake 3D / virtual surround effect

## Free vs Pro Messaging

### In-Extension (Popup UI)

When free user hits a Pro-only feature:

> Unlock full strength and all style presets — $4.99/month or $39.99/year.

### Store Listing Copy

**Short Description (max 132 chars):**

> One coffee a month — turn your YouTube and Spotify into studio-quality audio through your existing headphones.

**Free tier description:**

> Try Phaselith free with Reference preset at up to 50% strength. Hear the difference immediately — upgrade to Pro for the full experience.

**Pro tier description:**

> Phaselith Pro unlocks full strength control (0-100%), all 7 style presets (Reference, Grand, Smooth, Vocal, Punch, Air, Night), and complete HF reconstruction and dynamics control.

## User-Facing FAQ

### Is Phaselith free?

Phaselith has a free tier that lets you experience the core restoration engine with Reference preset at up to 50% strength. Pro unlocks full controls for $4.99/month or $39.99/year.

### Why not just make it free?

Building a real-time audio restoration engine that runs in a browser required years of R&D in signal processing, WebAssembly optimization, and psychoacoustic tuning. $4.99/month — less than a cup of coffee — supports continued development and new features.

### Can I cancel anytime?

Yes. Cancel anytime. Your subscription continues until the end of the billing period, then reverts to the free tier.

### Is there a lifetime option?

Not currently. We may introduce a lifetime license in the future based on user feedback.

## Tone Rules

When writing any public-facing text:

1. Do not oversell — say "restoration" not "lossless recovery"
2. Do not claim impossible guarantees
3. Do emphasize the coffee price analogy — it's relatable and disarming
4. Do emphasize the DAC comparison — it gives users a mental anchor for value
5. Do keep technical claims backed by real properties (idempotent, scale-invariant)

## Who Is This For

- 🎵 Music listeners who want richer sound from YouTube, Spotify, browser playback
- 🎙️ Podcast listeners who want clearer, more natural voices
- 🎧 Headphone users who want DAC-quality improvement without new hardware
- 💻 Anyone who notices browser audio sounds flat compared to dedicated players

## Strategic Note

The free tier exists to let users **hear the difference before paying**. The 50% strength cap is enough to notice improvement but not enough to feel satisfied — creating natural upgrade motivation without artificial annoyance.

The key conversion metric is: does the user hear a difference within 30 seconds of enabling the extension? If yes, the upgrade path sells itself.
