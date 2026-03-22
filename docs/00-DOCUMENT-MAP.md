# Phaselith Document Map

This directory is organized by function and reading order.

## Recommended Order

1. [01-PROJECT-SCOPE.md](01-PROJECT-SCOPE.md)
   What Phaselith is trying to do and what it is explicitly not trying to do.
2. [02-REPOSITORY-AND-RUNTIMES.md](02-REPOSITORY-AND-RUNTIMES.md)
   How the repository is split into core DSP, browser runtime, Windows APO runtime, and control surfaces.
3. [03-CORE-ALGORITHM.md](03-CORE-ALGORITHM.md)
   The main algorithm explanation and M0-M7 module contract.
4. [04-BROWSER-RUNTIME-FLOW.md](04-BROWSER-RUNTIME-FLOW.md)
   Browser path data flow, limits, and why it currently serves as the cleanest listening reference.
5. [05-WINDOWS-APO-RUNTIME-FLOW.md](05-WINDOWS-APO-RUNTIME-FLOW.md)
   Windows system-wide path, current transitional state, and target direction.
6. [06-CONTROLS-STYLES-AND-PRESETS.md](06-CONTROLS-STYLES-AND-PRESETS.md)
   Runtime controls, style intent, and product-facing control surface.
7. [07-VALIDATION-AND-LISTENING.md](07-VALIDATION-AND-LISTENING.md)
   Technical tests, listening regression workflow, and anti-self-deception checks.
8. [08-ROADMAP-AND-LIMITATIONS.md](08-ROADMAP-AND-LIMITATIONS.md)
   What is stable, what is transitional, and where the next major engineering work should go.
9. [09-LICENSING-AND-CONTRIBUTIONS.md](09-LICENSING-AND-CONTRIBUTIONS.md)
   Public licensing model, commercial licensing path, and contribution terms.
10. [10-DEFENSIVE-PUBLICATION.md](10-DEFENSIVE-PUBLICATION.md)
   Defensive-publication-style disclosure of the Phaselith technical idea space.
11. [11-CHROME-STORE-LAUNCH-AND-MONETIZATION.md](11-CHROME-STORE-LAUNCH-AND-MONETIZATION.md)
   Chrome Web Store launch copy, Free/Pro tier definition, and $4.99/month subscription pricing model.
12. [12-PUBLIC-V0.1-OPEN-SOURCE-PACKAGING.md](12-PUBLIC-V0.1-OPEN-SOURCE-PACKAGING.md)
   Public-release checklist, packaging rules, and what should or should not be included in the first open-source launch.
13. [13-BROWSER-TRANSIENT-REPAIR-AND-MACOS-FIX.md](13-BROWSER-TRANSIENT-REPAIR-AND-MACOS-FIX.md)
   Root cause, delayed-transient repair design, split transient probes, and the later browser sub-block policy (`Windows=1`, `macOS=1/8 popup override`) that keeps the best-sounding path available while giving macOS users a clear stability fallback.
14. [14-M2-STFT-ENGINE-CRACKLING.md](14-M2-STFT-ENGINE-CRACKLING.md)
   Root cause analysis of the M2 StftEngine crackling in Chrome AudioWorklet, workaround, and rules for future FFT path changes.
15. [15-APO-CHROME-COEXISTENCE.md](15-APO-CHROME-COEXISTENCE.md)
   APO and Chrome extension cannot coexist — passthrough APO causes Chrome audio crackling. Analysis, audio path diagram, and solution options.
16. [16-AUDIO-ARCHITECTURE-ANALYSIS.md](16-AUDIO-ARCHITECTURE-ANALYSIS.md)
   Audio architecture analysis.
17. [17-BUILD-PROFILES-AND-REALTIME-SAFETY.md](17-BUILD-PROFILES-AND-REALTIME-SAFETY.md)
   Why DSP crates must always compile with optimizations, even in dev mode. Per-crate profile overrides, industry practice, and the debug-build crackling root cause.
18. [18-PSYCHOACOUSTIC-MODULE-AND-ADAPTIVE-CONVERGENCE.md](18-PSYCHOACOUSTIC-MODULE-AND-ADAPTIVE-CONVERGENCE.md)
   Shared psychoacoustic module (Terhardt threshold, Schroeder spreading), M5 adaptive convergence with perceptual stopping criterion, and M6 masking refactor.
19. [19-OPTIMIZATION-SURVEY-AND-PATENT-FREE-ALGORITHMS.md](19-OPTIMIZATION-SURVEY-AND-PATENT-FREE-ALGORITHMS.md)
   Comprehensive M0–M6 optimization survey, limitations analysis, and 10 patent-free algorithms verified for commercial use.
20. [20-CEPSTRAL-ENVELOPE-AND-HIGHFIDELITY-MODE.md](20-CEPSTRAL-ENVELOPE-AND-HIGHFIDELITY-MODE.md)
   M4 cepstral spectral envelope (replacing power-law decay), HighFidelity QualityMode (decoupling FFT size from reprojection iterations), extreme dynamics protection, and perceptual comparison of power-law vs cepstral characteristics.
21. [21-DIRECTIONAL-HEADROOM-MIXING.md](21-DIRECTIONAL-HEADROOM-MIXING.md)
   M6 mixer directional headroom fix — corrected non-directional headroom formula and removed per-sample hard clamp that caused distortion during loud passages.
22. [22-APO-OLA-DISTORTION-ROOT-CAUSES.md](22-APO-OLA-DISTORTION-ROOT-CAUSES.md)
   All APO distortion root causes: M6 non-directional headroom, M6 per-sample hard clamp, M5 OLA multi-hop double-add, APO block > hop structural constraint, and sub-block engine fix.
