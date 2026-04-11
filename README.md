# open-bpm

High-accuracy BPM detection. Pure Rust. MIT license.

## Why

Every existing open-source BPM detector picks **one** tempo estimation method and hopes it works. open-bpm runs **six independent estimators** in parallel, fuses their results with metrical-aware clustering, then applies a **learned judge router** that corrects octave errors using a logistic regression trained on 1951 annotated tracks across three datasets.

### Two-Stage Architecture

**Stage 1 -- Signal Processing (pure, no learned parameters):**

| Estimator | What it measures | Why it helps |
|-----------|-----------------|--------------|
| IOI Histogram | Direct inter-onset interval measurement | Sub-BPM resolution from timing pairs |
| Comb Filter Bank | Resonance at beat periods | Robust when onsets are missing/noisy |
| Autocorrelation | Periodicity of onset envelope | Strong on steady rhythms |
| Low-Band AC | Kick-only autocorrelation (< 200 Hz) | Immune to triplet hi-hat confusion |
| Spectral Energy | FFT of RMS envelope | Independent from onset detection |
| Hopf Oscillator | Nonlinear resonator bank | Robust to missing beats and syncopation |

**Stage 2 -- Judge Router (learned, 32 features, 4 classes):**

A multinomial logistic regression decides whether to keep, halve, double, or triple the Stage 1 BPM. Trained on GiantSteps (EDM), Ballroom (dance), and GTZAN (10 genres). Only fires when confident (P > 0.65), otherwise preserves Stage 1. All weights are compile-time Rust constants -- zero external files, zero runtime dependencies.

## Accuracy

Evaluated on 2361 tracks across three standard MIR benchmarks:

| Dataset | Tracks | Genres | Acc1 (4% tol) | Acc2 (octave) |
|---------|--------|--------|---------------|---------------|
| GiantSteps | 664 | Electronic dance music | **68.8%** | 78.7% |
| Ballroom | 698 | 10 ballroom dance styles | **68.7%** | 87.1% |
| GTZAN | 999 | 10 genres (blues, classical, country, disco, hip-hop, jazz, metal, pop, reggae, rock) | **59.4%** | 83.3% |
| **Combined** | **2361** | | **64.9%** | **83.2%** |

The judge router improved Ballroom from 61.3% to 68.7% (+52 tracks) with zero regressions on GiantSteps. See [BENCHMARK.md](BENCHMARK.md) for full analysis including per-genre breakdowns, error distributions, and the 6-round optimization history.

## Install

```bash
cargo install open-bpm
```

From source:

```bash
git clone https://github.com/ASmallDuck/open-bpm
cd open-bpm
cargo build --release
```

## Usage

### CLI

```bash
open-bpm track.mp3                          # just the BPM
open-bpm --verbose track.flac               # per-estimator diagnostics
open-bpm --format json track.wav            # machine-readable
open-bpm --min-bpm 80 --max-bpm 180 track.ogg
```

Decodes WAV, MP3, FLAC, OGG, AAC via [Symphonia](https://github.com/pdeljanov/Symphonia).

### Library

```rust
use open_bpm::{detect, detect_with_options, DetectOptions};

let result = detect(&samples, 44100);
println!("{:.1} BPM (confidence: {:.0}%)", result.bpm, result.confidence * 100.0);
println!("First beat at {:.3}s", result.grid_offset);

// Custom range
let opts = DetectOptions {
    min_bpm: 80.0,
    max_bpm: 180.0,
    ..Default::default()
};
let result = detect_with_options(&samples, 44100, &opts);
```

The library depends only on `rustfft`. Audio decoding and CLI are behind the `cli` feature flag.

## Pipeline

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
             ├──► Spectral FFT ───┘         ▼
             └──► Hopf Oscillator ──► Tiebreaker (low confidence)
                                            │
                                            ▼
                                    Bar count + Phrase halving
                                            │
                                            ▼
                                    Judge Router (32-feat LR, 4 classes)
                                            │
                                            ▼
                                    Grid refinement ──► Integer snap ──► Result
```

See [OpenBPM.md](OpenBPM.md) for the full mathematical specification.

## Performance

Apple M-series, release build:

| Duration | Time |
|----------|------|
| 25 s | ~100 ms |
| 3 min | ~180 ms |
| 8 min | ~250 ms |

Binary size: ~2.4 MB (with Symphonia decoder). The judge router adds negligible overhead (~1 ms for 32 feature extractions + a 32x4 dot product).

## Benchmarking

Run the GiantSteps benchmark:

```bash
./bench/run_benchmark.sh
```

Additional datasets (require separate download):

```bash
./bench/run_ballroom_benchmark.sh    # 698 ballroom dance tracks
./bench/run_gtzan_benchmark.sh       # 999 tracks across 10 genres
```

## License

MIT
