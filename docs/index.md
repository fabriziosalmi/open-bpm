---
layout: home
title: OpenBPM — High-Accuracy BPM Detection in Pure Rust
titleTemplate: false

hero:
  name: "OpenBPM"
  text: "High-accuracy BPM detection in pure Rust."
  tagline: "Runs 6 independent estimators in parallel, fused with metrical-aware clustering and a learned judge router. Zero runtime dependencies."
  actions:
    - theme: brand
      text: Get Started
      link: /guide/introduction
    - theme: alt
      text: Technical Specification
      link: /guide/specification
    - theme: alt
      text: Benchmarks (2361 Tracks)
      link: /guide/benchmarks

features:
  - icon: ⚡
    title: Pure Rust Performance
    details: Ultra-fast signal processing with zero external files or C library dependencies. Core library relies only on rustfft.
  - icon: 🎯
    title: 6 Parallel Estimators
    details: Combines IOI Histogram, Comb Filter Bank, Autocorrelation, Low-Band AC, Spectral Energy, and Hopf Oscillator.
  - icon: 🧠
    title: Learned Judge Router
    details: Stage 2 multinomial logistic regression corrects octave errors (half/double/triple tempo) trained on 1951 annotated tracks.
  - icon: 📊
    title: 83.2% Octave-Accurate
    details: Rigorously evaluated across 2361 tracks on standard MIR benchmarks (GiantSteps, Ballroom, GTZAN).
  - icon: 🎧
    title: Multi-Format Audio
    details: CLI and optional decoding support for WAV, MP3, FLAC, OGG, and AAC via Symphonia.
  - icon: 📦
    title: Dual Mode (CLI & Lib)
    details: Usable as a standalone CLI tool or integrated directly into Rust audio applications and WASM pipelines.
---
