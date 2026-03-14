# 08. Roadmap and Limitations

## Current Stable Ground

The project currently has three strong anchors:

1. a real DSP core with deep test coverage
2. a browser runtime that serves as the cleanest listening reference
3. a Windows system-wide path that already proves the product direction

## Current Known Limitations

### Browser

1. not yet a fully stereo-native reconstruction runtime
2. currently better described as dual-mono processing with preserved stereo layout

### APO

1. current stereo execution model is transitional
2. not yet the final flagship stereo-native architecture

### Loudness

1. K-weighting currently uses 44.1k/48k coefficient support and falls back for higher rates
2. acceptable for browser-first work, but not yet a full high-rate reference implementation

## Algorithm Roadmap

### Near-Term

1. protect current center image strength
2. keep ambience preserve narrow and disciplined
3. continue validating across weak playback chains

### Mid-Term

1. improve stereo-native execution paths
2. complete the next stage of FFT/OLA work carefully
3. preserve current sound identity while strengthening generality

### Longer-Term

1. Windows flagship APO runtime
2. macOS Core Audio runtime
3. Linux PipeWire runtime

## FFT / OLA Work

The repository already contains planning work for deeper FFT/OLA architecture.

That work should stay disciplined:

1. define frame and hop semantics first
2. treat current sound as a regression baseline
3. do not replace the current sound blindly just because the new architecture looks cleaner on paper

## Platform Roadmap

### Windows

- final destination: stereo-native system-wide runtime

### macOS

- likely destination: Core Audio based runtime

### Linux

- likely destination: PipeWire based runtime

## Design Rule

The right next step is not automatically the most mathematically elegant step.

For CIRRUS, the right next step is the one that:

1. preserves the strongest current listening results
2. reduces architectural debt
3. increases runtime correctness
4. expands the effect to more real-world material
