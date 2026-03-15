# 11. Chrome Store Launch and Monetization

## Purpose

This document defines how CIRRUS should be introduced on the Chrome Web Store during the early product phase, how Early Access should be communicated, and how future paid unlocking should be announced without confusing users.

The goal is:

1. ship early
2. collect real listening feedback and reviews
3. avoid pretending the product is already a finished commercial release
4. prepare users for future paid unlocking without damaging trust

## Recommended Strategy

Use **one Chrome Web Store listing**.

Do not split the browser product into separate free and paid listings unless there is a later platform-level reason to do so.

Recommended rollout:

1. `Phase 1`
   - public Early Access
   - full functionality
   - free during the initial validation period
2. `Phase 2`
   - same listing remains live
   - free tier becomes limited
   - paid unlock is introduced through the official CIRRUS licensing flow
3. `Phase 3`
   - browser `Pro` unlock is sold at the target product price
   - desktop remains a separate higher-tier product line

This keeps:

1. installs concentrated in one listing
2. reviews concentrated in one listing
3. user confusion low
4. future upsell cleaner

## Product Positioning

CIRRUS should not be described as:

1. a simple EQ
2. a bass booster
3. a louder button
4. a fake 3D effect

It should be described as:

1. a real-time audio restoration and playback optimization engine
2. a way to reduce haze, masking, and roughness in browser playback
3. a tool that improves source focus, center image, and front placement
4. a premium-system presentation layer for everyday playback

## Store Name

Recommended:

`CIRRUS`

Optional longer variants if needed:

1. `CIRRUS Audio`
2. `CIRRUS for Chrome`
3. `CIRRUS Browser`

## Short Description Options

Use one of the following as the Chrome Web Store short description.

### Option A

`Real-time audio restoration and focus enhancement for browser playback.`

### Option B

`Reduce haze, improve center image, and make browser audio feel more front-focused.`

### Option C

`Premium-system presentation for everyday browser audio without acting like a simple EQ.`

Recommended default:

`Real-time audio restoration and focus enhancement for browser playback.`

## Full Description Draft

Use the following as the base long-form listing copy.

---

`CIRRUS is a real-time audio restoration and playback optimization engine for browser media.`

`Instead of acting like a fixed EQ curve, CIRRUS analyzes the playback signal and helps reduce masking, haze, roughness, and image instability so voices and instruments feel more focused, more intelligible, and more physically placed in front of the listener.`

`It is designed for real-world listening: streaming media, video platforms, compressed audio, laptop speakers, ordinary headphones, and everyday browser playback.`

### What CIRRUS is designed to improve

- reduce fog, harshness, and codec-like roughness
- improve center image and front-focused source placement
- make speech and vocals feel more physically locked in place
- preserve listenability instead of turning everything into an exaggerated effect

### What CIRRUS is not

- not a conventional EQ
- not a simple widening effect
- not just a loudness enhancer
- not a fake reverb or “make everything huge” processor

### Best use cases

- YouTube and browser video
- compressed or underwhelming streaming playback
- laptop speakers
- everyday headphones and desktop listening

### Current release status

`This release is part of the CIRRUS Early Access phase.`

`The goal of Early Access is to collect real listening feedback, validate use cases, and refine the product before formal paid unlocking begins.`

`Features, tuning, and product packaging may evolve over time.`

---

## Early Access Notice

This text should appear both:

1. in the Chrome Store description
2. in the extension popup or onboarding UI

Recommended copy:

`CIRRUS is currently in Early Access.`

`During this phase, the browser version is available for evaluation while we collect listening feedback, stabilize the experience, and refine the final product tiers.`

`This is not the final commercial packaging of CIRRUS.`

## Future Paid Unlock Notice

The goal is to be honest without creating fear or backlash.

Recommended copy:

`CIRRUS is currently available during its Early Access period.`

`In a future release, CIRRUS browser features will move to a trial + free + paid unlock model.`

`The current Early Access release is intended to gather real-world feedback before paid unlocking begins.`

`Pricing and feature tiers will be announced in advance inside the extension and on official CIRRUS pages.`

## Recommended Pricing Messaging

Do not announce aggressive monetization on day one.

Recommended sequence:

1. launch as free Early Access
2. state clearly that a future trial + free + paid structure is planned
3. only announce the specific browser price once the product feels stable

Current intended product direction:

1. `CIRRUS Browser`
   - launch / Early Access conversion price: `USD 49`
   - normal personal price target: `USD 59`
   - acceptable long-term ceiling if value perception stays strong: `USD 69`
2. `CIRRUS Desktop`
   - target price: `USD 199`
3. `Commercial licensing`
   - separate contact-based pricing
   - for closed-source embedding, redistribution, OEM, and team/commercial deployment

Do not put both numbers in the initial public store copy unless you are ready to support them.

## Recommended Monetization Structure

The browser product should not jump directly from free Early Access to a hard paywall.

Recommended structure:

1. `Trial`
   - `30 days`
   - full browser experience
   - all styles available
   - full strength range available
2. `Free`
   - after trial ends
   - `Reference` only
   - `Strength` capped at `50%`
   - intended to remain genuinely useful, not crippled
3. `Paid personal unlock`
   - one-time purchase
   - unlocks all styles
   - unlocks full strength range
   - unlocks future browser updates for the paid browser product line
4. `Desktop`
   - separate one-time purchase
   - positioned as the higher-tier system-level product

This creates:

1. low friction at install time
2. a fair hearing window for a listening-first product
3. a meaningful free mode after trial
4. a clear reason to pay without making the free mode feel fake

## Feature Split

Recommended browser tier split:

### Trial

1. all browser styles available
2. full `Strength` range
3. full current browser controls
4. no sound-quality limitation relative to the paid browser version

### Free After Trial

1. style locked to `Reference`
2. `Strength` capped at `50%`
3. existing basic browser playback improvement remains usable
4. should continue to demonstrate the core CIRRUS value clearly

### Paid Browser Unlock

1. all style presets available
2. full `Strength` range
3. all current browser controls available
4. official packaged build, updates, and personal-use license

### Commercial License

Commercial licensing should be presented as a separate path, not as a more expensive version of the personal unlock.

It should cover:

1. closed-source integration
2. redistribution
3. OEM or hardware bundling
4. team deployment
5. any use that does not fit the normal personal paid browser license

## Commercial License Entry

The commercial licensing entry must be visible in all official surfaces where serious users may look for purchase information.

Recommended locations:

1. project README
2. official website or landing page
3. extension upgrade screen
4. Chrome Web Store description

Recommended wording:

`Need CIRRUS for a product, closed-source integration, redistribution, or commercial deployment?`

`Contact CIRRUS for commercial licensing.`

Keep the personal and commercial paths visually separate:

1. `Buy Personal License`
2. `Commercial Licensing`

## Trial-To-Paid Conversion Design

The key conversion rule is:

`Do not let the trial ending feel like punishment.`

Recommended conversion behavior:

1. before trial ends, show advance notice inside the extension
2. when trial ends, move the user into `Free` automatically
3. explain clearly what remains available
4. explain clearly what unlocks with payment
5. make the paid action a one-click next step from inside the extension

Recommended timeline:

1. `7 days remaining`
   - light notice only
2. `3 days remaining`
   - clearer reminder
3. `1 day remaining`
   - direct explanation of the upcoming free-mode limits
4. `trial ended`
   - confirm user now has `Reference + 50% Strength`
   - offer paid unlock

## Recommended Trial/Upgrade Copy

### Trial Running

`Early Access Trial`

`You are currently hearing the full CIRRUS browser experience.`

`After the trial, CIRRUS will remain available in Free mode with Reference style and up to 50% Strength.`

### Trial Ending Soon

`Your full CIRRUS trial ends in {X} days.`

`After that, CIRRUS will continue in Free mode with Reference style and up to 50% Strength.`

`Unlock the full browser version to keep all styles and full intensity.`

### Trial Ended

`Your full CIRRUS trial has ended.`

`CIRRUS remains active in Free mode with Reference style and up to 50% Strength.`

`Unlock the full browser version to restore all styles and the full Strength range.`

### Upgrade CTA

`Unlock Full CIRRUS`

`Get all styles, full Strength, and the complete browser experience with a one-time purchase.`

### Commercial CTA

`Need CIRRUS for a product or commercial deployment?`

`Contact us for commercial licensing.`

## Pricing Positioning Guidance

The browser price should not be defended as "cheap software."

It should be framed as:

1. a one-time purchase for a product that changes day-to-day listening
2. materially cheaper than replacing audio hardware
3. a premium listening tool, not a novelty effect

Do not say:

1. `It's just a plugin`
2. `It's cheaper than coffee`
3. `It's basically free`

Better positioning:

1. `A one-time unlock for the full browser listening experience`
2. `Much less expensive than replacing hardware just to recover the same degree of improvement`
3. `Built for listeners who can hear the difference immediately`

## Recommended User-Facing FAQ Copy

### Is CIRRUS free?

`CIRRUS is currently available as an Early Access release.`

`A future trial + free + paid unlock model is planned, but paid unlocking has not started yet in this release phase.`

### Will CIRRUS become paid later?

`Yes.`

`The browser product is planned to move to a trial + free + paid unlock structure after the Early Access phase.`

### Why release it free first?

`Because CIRRUS is a listening-first product.`

`The Early Access phase helps us validate real-world listening results, improve stability, and refine the final feature set before formal paid unlocking begins.`

## Extension Popup Copy

Use this in-product text for the Early Access phase:

`Early Access`

`CIRRUS is currently available for evaluation while we refine the product and gather listening feedback.`

`Future releases will introduce a full trial period, a usable Free mode, and a paid full unlock.`

## Later In-Product Upgrade Copy

Use this once paid unlocking is ready:

`CIRRUS Pro unlock`

`The Early Access phase has ended.`

`Continue using CIRRUS Free with Reference style and up to 50% Strength, or unlock the full browser experience.`

## Tone Rules

When writing any public-facing Chrome Store text:

1. do not oversell it as proven “lossless recovery”
2. do not claim impossible technical guarantees
3. do emphasize focus, clarity, front placement, and premium presentation
4. do state that the product is still evolving
5. do keep the copy consistent with the real current browser runtime

## Internal Launch Checklist

Before public launch:

1. confirm permissions text is accurate
2. confirm tab-follow behavior is stable enough for normal use
3. confirm volume no longer feels obviously disadvantaged
4. confirm the extension explains Early Access clearly
5. confirm future paid unlocking language is present but not hostile

## Strategic Note

The most important thing to validate first is not payment infrastructure.

It is whether real users repeatedly hear the same thing the current internal listening process hears:

1. stronger center image
2. more front-focused placement
3. less haze and roughness
4. a premium-system presentation effect on ordinary playback

That is why the preferred launch order is:

1. public Early Access
2. listening validation and product tightening
3. paid unlocking
