# 06. Controls, Styles, and Presets

## Engine Controls

The engine currently exposes two classes of controls:

1. structural controls
2. presentation controls

### Structural Controls

- `enabled`
- `strength`
- `hf_reconstruction`
- `dynamics`
- `transient`
- `phase_mode`
- `quality_mode`
- `synthesis_mode`

These controls influence how aggressively Phaselith tries to detect, reconstruct, and validate repairs.

### Presentation Controls

- `style`
- `warmth`
- `air_brightness`
- `smoothness`
- `spatial_spread`
- `impact_gain`
- `body`
- `ambience_preserve`

These controls shape how the repaired signal is finally presented.

## Why `ambience_preserve` Is Separate

`ambience_preserve` must remain independent from `spatial_spread`.

Why:

1. width and ambience are not the same thing
2. width is a stereo-layout decision
3. ambience preserve is a dry-vs-tail compensation decision
4. combining them makes tuning and product behavior harder to reason about

## Style Philosophy

Styles should not be random tone presets.

Each style should represent a stable presentation personality built on the same Phaselith core.

Typical style directions:

1. `Reference`
   Balanced, disciplined, least decorative
2. `Grand`
   More expansive and luxurious presentation
3. `Smooth`
   Reduced edge and roughness
4. `Vocal`
   Front-focused voice intimacy
5. `Punch`
   More attack and density
6. `Air`
   More openness and upper-band lift in presentation
7. `Night`
   Softer, more intimate, less aggressive presentation

## Control Rules

1. Do not let style controls become disguised loudness tricks.
2. Do not use styles to hide structural problems in the core algorithm.
3. Do not allow ambience recovery to destroy center image.
4. Do not let "more impressive" become "less believable."

## Product Rule

The browser product can ship with fewer visible controls than the desktop product.

That is not because browser matters less.

It is because strong product design often means:

1. simple top-level controls
2. deep internal structure
3. stable sound identity
