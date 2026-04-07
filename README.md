# open-bpm

High-accuracy BPM detection. Pure Rust. No ML models. MIT license.

## Why

Every existing open-source BPM detector picks **one** tempo estimation method and hopes it works. open-bpm runs **three independent estimators** in parallel and fuses their results — the same signal can't fool all three the same way.

| Estimator | What it measures | Why it helps |
|-----------|-----------------|--------------|
| IOI Histogram | Direct inter-onset interval measurement | Sub-BPM resolution from timing pairs |
| Comb Filter Bank | Resonance at beat periods | Robust when onsets are missing/noisy |
| Autocorrelation | Periodicity of onset envelope | Strong on steady rhythms |

When 2+ estimators agree: weighted average. When they disagree: highest confidence wins. When all three agree: confidence bonus.

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
             ├──► Comb Filter ────┤──► Fusion ──► Octave resolution
             └──► Autocorrelation ┘         │
                                            ▼
                                    Fine refinement (±2.5 BPM @ 0.1 step)
                                            │
                                            ▼
                                    PLL grid offset ──► Integer snap ──► Result
```

See [OpenBPM.md](OpenBPM.md) for the full mathematical specification.

## Performance

Apple M-series, release build:

| Duration | Time |
|----------|------|
| 25 s | ~100 ms |
| 3 min | ~180 ms |
| 8 min | ~250 ms |

Binary size: ~2.4 MB (with Symphonia decoder).

## Accuracy

See [PROGRESS.md](PROGRESS.md) for benchmark results and iteration history.

## License

MIT
