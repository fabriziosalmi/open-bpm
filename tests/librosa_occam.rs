//! librosa-occam: synthetic tests targeting the 31 tracks where librosa beats us.
//!
//! Pattern analysis of our losses:
//!   4/3x errors: 9 tracks (triplet hi-hats, gt 110-140)
//!   other: 12 tracks (various, mostly small errors ~5-10%)
//!   0.5x: 4 tracks (octave down on slow tracks, gt 90-140)
//!   2x: 4 tracks (octave up on slow tracks, gt 80-101)
//!   2/3x: 2 tracks (combined)
//!
//! Strategy: generate signals matching these BPMs with realistic EDM patterns,
//! then fix what fails.

use open_bpm::detect;
use std::f64::consts::PI;

// ============================================================================
// Generators (reuse concepts from synthetic_edm.rs)
// ============================================================================

fn sine_decay(freq: f64, samples: usize, sr: f64) -> Vec<f32> {
    (0..samples).map(|i| {
        let t = i as f64 / sr;
        ((2.0 * PI * freq * t).sin() * (-(i as f64) / (samples as f64 * 0.2)).exp()) as f32
    }).collect()
}

fn noise_decay(samples: usize, sr: f64, seed: u64) -> Vec<f32> {
    let mut state = seed;
    (0..samples).map(|i| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let noise = ((state >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0;
        let decay = (-(i as f64) / (samples as f64 * 0.15)).exp();
        (noise * decay) as f32
    }).collect()
}

fn place(buf: &mut [f32], pos: usize, sound: &[f32], amp: f32) {
    for (i, &s) in sound.iter().enumerate() {
        if pos + i < buf.len() { buf[pos + i] += s * amp; }
    }
}

/// Standard 4/4 kick pattern — the baseline that librosa gets right.
fn four_on_floor(bpm: f64, sr: u32, dur: f64) -> Vec<f32> {
    let s = sr as f64;
    let n = (dur * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let kick = sine_decay(55.0, (s * 0.05) as usize, s);
    let hat = noise_decay((s * 0.008) as usize, s, 42);
    let mut buf = vec![0.0f32; n];
    let mut pos = 0;
    while pos < n {
        place(&mut buf, pos, &kick, 0.8);
        // Hat on every 8th note
        place(&mut buf, pos, &hat, 0.3);
        let eighth = pos + beat / 2;
        if eighth < n { place(&mut buf, eighth, &hat, 0.25); }
        pos += beat;
    }
    buf
}

/// Slow track with clear downbeat — targets our 2x octave-up errors.
/// Key: strong kick every beat + soft melodic element between beats.
fn slow_with_melody(bpm: f64, sr: u32, dur: f64) -> Vec<f32> {
    let s = sr as f64;
    let n = (dur * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let kick = sine_decay(50.0, (s * 0.06) as usize, s);
    let melody = sine_decay(440.0, (s * 0.1) as usize, s);
    let hat = noise_decay((s * 0.006) as usize, s, 77);
    let mut buf = vec![0.0f32; n];
    let mut pos = 0;
    let mut beat_n = 0;
    while pos < n {
        place(&mut buf, pos, &kick, 0.7);
        // Soft melody on off-beats (creates rhythmic content without doubling)
        let offbeat = pos + beat / 2;
        if offbeat < n {
            place(&mut buf, offbeat, &melody, 0.15);
            place(&mut buf, offbeat, &hat, 0.2);
        }
        pos += beat;
        beat_n += 1;
    }
    let _ = beat_n;
    buf
}

/// Track with triplet subdivisions — targets our 4/3x errors.
/// Strong kick on quarters, triplet hi-hats, clear snare on 2&4.
fn triplet_groove(bpm: f64, sr: u32, dur: f64) -> Vec<f32> {
    let s = sr as f64;
    let n = (dur * s) as usize;
    let beat = 60.0 / bpm * s;
    let triplet = beat / 3.0;
    let kick = sine_decay(55.0, (s * 0.05) as usize, s);
    let snare = sine_decay(200.0, (s * 0.03) as usize, s);
    let hat = noise_decay((s * 0.006) as usize, s, 99);
    let mut buf = vec![0.0f32; n];
    // Kick on every beat
    let mut pos = 0.0;
    let mut beat_n = 0;
    while (pos as usize) < n {
        place(&mut buf, pos as usize, &kick, 0.8);
        if beat_n % 4 == 1 || beat_n % 4 == 3 {
            place(&mut buf, pos as usize, &snare, 0.6);
        }
        pos += beat;
        beat_n += 1;
    }
    // Triplet hats — quieter than kicks
    pos = 0.0;
    while (pos as usize) < n {
        place(&mut buf, pos as usize, &hat, 0.35);
        pos += triplet;
    }
    buf
}

const TOL: f64 = 4.0;  // BPM tolerance
const TOL_TIGHT: f64 = 2.0;
const SR: u32 = 44100;
const DUR: f64 = 20.0;

// ============================================================================
// Tests targeting our weak spots (the 31 librosa-only wins)
// ============================================================================

/// Slow BPMs where we octave-double (gt < 100)
#[test]
fn slow_bpm_no_doubling() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[80.0, 85.0, 90.0, 95.0, 99.0, 101.0] {
        let samples = slow_with_melody(bpm, SR, DUR);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= TOL { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "Slow BPM doubling: {:?}", fails);
}

/// Standard 4/4 at 110-140 — our core range, must be perfect
#[test]
fn core_range_perfect() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[110.0, 115.0, 119.0, 120.0, 125.0, 126.0, 128.0, 130.0, 132.0, 135.0, 140.0] {
        let samples = four_on_floor(bpm, SR, DUR);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= 2.0 { fails.push((bpm, r.bpm)); } // tighter tolerance
    }
    assert!(fails.is_empty(), "Core range failures: {:?}", fails);
}

/// Triplet groove — must detect quarter-note BPM, not 4/3
#[test]
fn triplet_no_43_error() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[110.0, 120.0, 126.0, 128.0, 132.0, 140.0] {
        let samples = triplet_groove(bpm, SR, DUR);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= TOL { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "Triplet 4/3 errors: {:?}", fails);
}

/// Mid-range BPMs (150-160) where our "other" errors cluster
#[test]
fn mid_fast_range() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[150.0, 155.0, 158.0, 160.0] {
        let samples = four_on_floor(bpm, SR, DUR);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= 2.0 { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "Mid-fast range failures: {:?}", fails);
}

// ============================================================================
// NEGATIVE TESTS — things that already work, must NOT regress
// ============================================================================

/// DnB range — our biggest strength vs librosa. Do NOT break these.
#[test]
fn negative_dnb_must_stay_correct() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[170.0, 172.0, 174.0, 176.0, 180.0] {
        let samples = four_on_floor(bpm, SR, 15.0);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= TOL_TIGHT { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "DnB REGRESSION: {:?}", fails);
}

/// Standard techno range — our bread and butter. Do NOT break.
#[test]
fn negative_techno_must_stay_correct() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[128.0, 130.0, 133.0, 135.0, 138.0, 140.0, 145.0] {
        let samples = four_on_floor(bpm, SR, 15.0);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= TOL_TIGHT { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "Techno REGRESSION: {:?}", fails);
}

/// House range — core accuracy. Do NOT break.
#[test]
fn negative_house_must_stay_correct() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[118.0, 120.0, 122.0, 124.0, 126.0] {
        let samples = four_on_floor(bpm, SR, 15.0);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= TOL_TIGHT { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "House REGRESSION: {:?}", fails);
}

/// Triplet patterns at standard BPMs — already working. Do NOT break.
#[test]
fn negative_triplet_standard_must_stay() {
    let mut fails: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[120.0, 126.0, 128.0, 140.0] {
        let samples = triplet_groove(bpm, SR, DUR);
        let r = detect(&samples, SR);
        if (r.bpm - bpm).abs() >= TOL { fails.push((bpm, r.bpm)); }
    }
    assert!(fails.is_empty(), "Triplet REGRESSION: {:?}", fails);
}
