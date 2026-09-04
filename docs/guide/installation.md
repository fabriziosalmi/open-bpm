# Installation & CLI Usage

OpenBPM can be installed as a standalone CLI utility or integrated as a library dependency in your Rust projects.

---

## Installing the CLI

### Via Cargo

```bash
cargo install open-bpm
```

### From Source

```bash
git clone https://github.com/ASmallDuck/open-bpm
cd open-bpm
cargo build --release
```

The compiled binary will be available at `./target/release/open-bpm`.

---

## CLI Usage

### Basic Tempo Detection

Output only the detected BPM:

```bash
open-bpm track.mp3
# Output: 128.0
```

### Detailed Diagnostics

Inspect individual estimator outputs and confidence scores:

```bash
open-bpm --verbose track.flac
```

### Machine-Readable Output (JSON)

Generate JSON for scripts, automation, and audio pipelines:

```bash
open-bpm --format json track.wav
```

Sample JSON response:

```json
{
  "bpm": 124.02,
  "confidence": 0.91,
  "grid_offset": 0.124,
  "stage1_bpm": 124.02,
  "router_action": "keep",
  "router_confidence": 0.88
}
```

### Constraining the Search Range

For specialized genres (e.g. ambient or drum & bass), customize the search range:

```bash
open-bpm --min-bpm 80 --max-bpm 180 track.ogg
```

---

## Supported Audio Codecs

OpenBPM leverages [Symphonia](https://github.com/pdeljanov/Symphonia) for native, pure Rust audio decoding:
- **WAV** (PCM 16/24/32-bit, float)
- **MP3** (MPEG-1/2 Audio Layer III)
- **FLAC** (Free Lossless Audio Codec)
- **OGG** (Vorbis)
- **AAC** (Advanced Audio Coding)

---

## Using as a Rust Library

Add to your `Cargo.toml`:

```toml
[dependencies]
open-bpm = "0.1"
```

In your code:

```rust
use open_bpm::{detect, detect_with_options, DetectOptions};

fn main() {
    let samples: Vec<f32> = load_mono_samples();
    let sample_rate = 44100;

    // Standard detection
    let result = detect(&samples, sample_rate);
    println!("{:.1} BPM (confidence: {:.0}%)", result.bpm, result.confidence * 100.0);
    println!("First beat at {:.3}s", result.grid_offset);

    // Custom options
    let opts = DetectOptions {
        min_bpm: 80.0,
        max_bpm: 180.0,
        ..Default::default()
    };
    let custom_result = detect_with_options(&samples, sample_rate, &opts);
}
```
