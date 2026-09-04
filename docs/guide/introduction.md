# Introduction to OpenBPM

**OpenBPM** is an open-source, high-accuracy BPM (tempo) detection engine written in pure Rust.

Most existing tempo estimation tools pick a single algorithmic method (such as simple peak detection or basic autocorrelation) and hope it works across different musical genres. OpenBPM takes an ensemble approach: it executes **six independent signal processing estimators** in parallel, clusters their hypotheses, and uses a **learned judge router** to resolve octave ambiguities.

---

## The Two-Stage Architecture

```
Audio ──► Multi-band filter (low/mid/high)
             │
             ▼
         SuperFlux onset detection (per band)
             │
             ▼
         Weighted merge (kick=2x, snare=1.5x, hat=0.5x)
             │
             ├──► IOI Histogram ──┐
             ├──► Comb Filter ────┤
             ├──► Autocorrelation ┤──► Metrical fusion ──► Octave resolution
             ├──► Low-Band AC ────┤         │
             ├──► Spectral Energy ┘         ▼
             └──► Hopf Oscillator     Judge Router (learned)
                                            │
                                            ▼
                                     Final BPM + Confidence
```

### Stage 1: Pure Signal Processing

Stage 1 uses pure signal processing without learned parameters:

| Estimator | What it measures | Why it helps |
| :--- | :--- | :--- |
| **IOI Histogram** | Direct inter-onset interval measurement | Sub-BPM resolution from timing pairs |
| **Comb Filter Bank** | Resonance at beat periods | Robust when onsets are missing or noisy |
| **Autocorrelation** | Periodicity of onset envelope | Strong on steady, predictable rhythms |
| **Low-Band AC** | Kick-only autocorrelation (< 200 Hz) | Immune to triplet hi-hat or syncopated snare confusion |
| **Spectral Energy** | FFT of RMS envelope | Completely independent from onset detection |
| **Hopf Oscillator** | Nonlinear resonator bank | Robust against syncopation and missing downbeats |

### Stage 2: Learned Judge Router

A multinomial logistic regression model (32 features, 4 classes) decides whether to **keep**, **halve**, **double**, or **triple** the Stage 1 BPM estimate:
- Trained on GiantSteps (EDM), Ballroom (dance), and GTZAN (10 genres).
- Only triggers octave adjustment when prediction probability exceeds confidence threshold ($P > 0.65$).
- Weights are embedded directly as compile-time Rust constants: **zero external files, zero runtime dependencies**.
