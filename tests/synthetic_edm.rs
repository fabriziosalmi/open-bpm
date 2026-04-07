//! Synthetic EDM pattern tests.
//!
//! These signals reproduce the specific rhythmic patterns that cause errors
//! on real EDM audio (GiantSteps benchmark). Each test targets one error class
//! from the error analysis.
//!
//! Unlike the draconian tests (clean clicks), these generate realistic
//! multi-layer drum patterns: kick + snare + hi-hat + sub-bass.

use open_bpm::{detect, detect_with_options, DetectOptions};
use std::f64::consts::PI;

// ============================================================================
// Signal generators — realistic EDM drum synthesis
// ============================================================================

/// Exponentially decaying sine burst.
fn sine_burst(freq: f64, duration_samples: usize, sr: f64) -> Vec<f32> {
    let decay_rate = 5.0 / duration_samples as f64; // 5 time constants
    (0..duration_samples)
        .map(|i| {
            let t = i as f64 / sr;
            let decay = (-decay_rate * i as f64).exp();
            (2.0 * PI * freq * t).sin() as f32 * decay as f32
        })
        .collect()
}

/// Noise burst (hi-hat like).
fn noise_burst(duration_samples: usize, sr: f64) -> Vec<f32> {
    let decay_rate = 8.0 / duration_samples as f64;
    let mut state: u64 = 0xCAFEBABE;
    (0..duration_samples)
        .map(|i| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let noise = ((state >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0;
            let decay = (-decay_rate * i as f64).exp();
            // High-pass character: multiply by high-freq sine
            let hp = (2.0 * PI * 8000.0 * i as f64 / sr).sin();
            (noise * hp * decay) as f32
        })
        .collect()
}

/// Place a sound at a position in the buffer, with given amplitude.
fn place(buffer: &mut [f32], pos: usize, sound: &[f32], amp: f32) {
    for (i, &s) in sound.iter().enumerate() {
        if pos + i < buffer.len() {
            buffer[pos + i] += s * amp;
        }
    }
}

/// Generate a standard 4/4 EDM pattern.
///
/// - Kick on every beat (quarter notes)
/// - Snare on beats 2 and 4
/// - Hi-hat on every 8th note
/// - Sub-bass following kick
fn standard_4_4(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let eighth = beat / 2;

    let kick = sine_burst(55.0, (s * 0.05) as usize, s);
    let snare = sine_burst(200.0, (s * 0.03) as usize, s);
    let hat = noise_burst((s * 0.01) as usize, s);
    let sub = sine_burst(40.0, (s * 0.08) as usize, s);

    let mut buf = vec![0.0f32; total];
    let mut pos = 0;
    let mut beat_in_bar = 0;
    while pos < total {
        // Kick on every beat
        place(&mut buf, pos, &kick, 0.8);
        place(&mut buf, pos, &sub, 0.5);

        // Snare on 2 and 4
        if beat_in_bar == 1 || beat_in_bar == 3 {
            place(&mut buf, pos, &snare, 0.6);
        }

        // Hi-hat on every 8th
        place(&mut buf, pos, &hat, 0.3);
        if pos + eighth < total {
            place(&mut buf, pos + eighth, &hat, 0.25);
        }

        pos += beat;
        beat_in_bar = (beat_in_bar + 1) % 4;
    }
    buf
}

// ============================================================================
// Pattern A: Triplet hi-hats (causes 4/3 error)
//
// Kick on quarter notes at BPM, but hi-hats on triplet 8ths (3 per beat).
// The triplet creates a strong periodicity at BPM * 4/3.
// The detector should still report the kick BPM, not the triplet BPM.
// ============================================================================

fn triplet_hihat_pattern(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = 60.0 / bpm * s;
    let triplet = beat / 3.0; // hi-hat triplet subdivision

    let kick = sine_burst(55.0, (s * 0.05) as usize, s);
    let snare = sine_burst(200.0, (s * 0.03) as usize, s);
    let hat = noise_burst((s * 0.008) as usize, s);
    let sub = sine_burst(40.0, (s * 0.08) as usize, s);

    let mut buf = vec![0.0f32; total];

    // Kicks on quarter notes
    let mut pos = 0.0;
    let mut beat_count = 0;
    while (pos as usize) < total {
        place(&mut buf, pos as usize, &kick, 0.8);
        place(&mut buf, pos as usize, &sub, 0.5);
        if beat_count % 4 == 1 || beat_count % 4 == 3 {
            place(&mut buf, pos as usize, &snare, 0.6);
        }
        pos += beat;
        beat_count += 1;
    }

    // Hi-hats on triplets (3 per beat) — LOUDER than in standard pattern
    pos = 0.0;
    while (pos as usize) < total {
        place(&mut buf, pos as usize, &hat, 0.5);
        pos += triplet;
    }

    buf
}

// ============================================================================
// Pattern B: Half-time groove (causes 0.5x error)
//
// Kick only on beats 1 and 3 (every other beat), snare on beat 3.
// Hi-hats on 8th notes keep the actual tempo visible.
// Classic dubstep/trap half-time feel at the real BPM.
// ============================================================================

fn halftime_pattern(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let eighth = beat / 2;

    let kick = sine_burst(50.0, (s * 0.06) as usize, s);
    let snare = sine_burst(180.0, (s * 0.04) as usize, s);
    let hat = noise_burst((s * 0.008) as usize, s);
    let sub = sine_burst(35.0, (s * 0.1) as usize, s);

    let mut buf = vec![0.0f32; total];
    let mut pos = 0;
    let mut beat_in_bar = 0;
    while pos < total {
        // Kick only on 1 and 3 (half-time feel)
        if beat_in_bar == 0 || beat_in_bar == 2 {
            place(&mut buf, pos, &kick, 0.9);
            place(&mut buf, pos, &sub, 0.6);
        }

        // Snare only on 3 (half-time backbeat)
        if beat_in_bar == 2 {
            place(&mut buf, pos, &snare, 0.7);
        }

        // Hi-hats keep the real tempo visible
        place(&mut buf, pos, &hat, 0.3);
        if pos + eighth < total {
            place(&mut buf, pos + eighth, &hat, 0.2);
        }

        pos += beat;
        beat_in_bar = (beat_in_bar + 1) % 4;
    }
    buf
}

// ============================================================================
// Pattern C: Offbeat hi-hats (causes 2x error)
//
// Kick on quarter notes, but hi-hats ONLY on the offbeats (the "and"s).
// This makes the 8th-note rate more prominent than the quarter note.
// Classic house pattern. Detector should report quarter-note BPM.
// ============================================================================

fn offbeat_hihat_pattern(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let eighth = beat / 2;

    let kick = sine_burst(55.0, (s * 0.05) as usize, s);
    let hat = noise_burst((s * 0.01) as usize, s);

    let mut buf = vec![0.0f32; total];
    let mut pos = 0;
    while pos < total {
        // Kick on every quarter note
        place(&mut buf, pos, &kick, 0.8);
        // Hi-hat ONLY on offbeats — louder than kick
        if pos + eighth < total {
            place(&mut buf, pos + eighth, &hat, 0.9);
        }
        pos += beat;
    }
    buf
}

// ============================================================================
// Pattern D: Sparse kick + dense hats (causes 2/3 error = half + triplet)
//
// Kick every 2 beats (half-time), triplet hi-hats.
// Combines both error sources: half-time AND triplet.
// ============================================================================

fn sparse_kick_triplet_hat(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = 60.0 / bpm * s;
    let triplet = beat / 3.0;

    let kick = sine_burst(50.0, (s * 0.06) as usize, s);
    let hat = noise_burst((s * 0.006) as usize, s);
    let sub = sine_burst(35.0, (s * 0.1) as usize, s);

    let mut buf = vec![0.0f32; total];

    // Kick every 2 beats
    let mut pos = 0.0;
    while (pos as usize) < total {
        place(&mut buf, pos as usize, &kick, 0.9);
        place(&mut buf, pos as usize, &sub, 0.5);
        pos += beat * 2.0;
    }

    // Triplet hi-hats
    pos = 0.0;
    while (pos as usize) < total {
        place(&mut buf, pos as usize, &hat, 0.5);
        pos += triplet;
    }

    buf
}

// ============================================================================
// Pattern E: Syncopated kick (realistic techno)
//
// Kick on 1, and-of-2, 3 (syncopated). Snare on 2, 4.
// Hi-hat on every 16th note. Classic techno groove.
// Tests whether syncopation confuses the IOI histogram.
// ============================================================================

fn syncopated_techno(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let sixteenth = beat / 4;

    let kick = sine_burst(52.0, (s * 0.05) as usize, s);
    let snare = sine_burst(190.0, (s * 0.03) as usize, s);
    let hat = noise_burst((s * 0.005) as usize, s);

    let mut buf = vec![0.0f32; total];
    let mut pos = 0;
    let mut beat_in_bar = 0;
    while pos < total {
        // Syncopated kick: beat 1, and-of-2, beat 3
        if beat_in_bar == 0 || beat_in_bar == 2 {
            place(&mut buf, pos, &kick, 0.8);
        }
        if beat_in_bar == 1 {
            // and-of-2 = halfway through beat 2
            let and_pos = pos + beat / 2;
            if and_pos < total {
                place(&mut buf, and_pos, &kick, 0.7);
            }
        }

        // Snare on 2 and 4
        if beat_in_bar == 1 || beat_in_bar == 3 {
            place(&mut buf, pos, &snare, 0.6);
        }

        // 16th note hi-hats
        for i in 0..4 {
            let hat_pos = pos + i * sixteenth;
            if hat_pos < total {
                place(&mut buf, hat_pos, &hat, 0.25);
            }
        }

        pos += beat;
        beat_in_bar = (beat_in_bar + 1) % 4;
    }
    buf
}

// ============================================================================
// Pattern F: Minimal techno (sparse, low energy)
//
// Kick every beat but soft. No snare. Very quiet hi-hat every 2 beats.
// Lots of silence. Tests low-energy track detection.
// ============================================================================

fn minimal_techno(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = (60.0 / bpm * s) as usize;

    let kick = sine_burst(48.0, (s * 0.04) as usize, s);
    let hat = noise_burst((s * 0.003) as usize, s);

    let mut buf = vec![0.0f32; total];
    let mut pos = 0;
    let mut beat_count = 0;
    while pos < total {
        place(&mut buf, pos, &kick, 0.4); // soft kick
        if beat_count % 2 == 0 {
            place(&mut buf, pos, &hat, 0.1); // very soft hat
        }
        pos += beat;
        beat_count += 1;
    }
    buf
}

// ============================================================================
// Pattern G: DnB two-step
//
// Kick on beat 1, snare on beat 2. Fast tempo (170+).
// Hi-hats on 8ths. The classic 2-step DnB pattern.
// Tests octave stability at high BPM.
// ============================================================================

fn dnb_twostep(bpm: f64, sr: u32, duration_secs: f64) -> Vec<f32> {
    let s = sr as f64;
    let total = (duration_secs * s) as usize;
    let beat = (60.0 / bpm * s) as usize;
    let eighth = beat / 2;

    let kick = sine_burst(55.0, (s * 0.04) as usize, s);
    let snare = sine_burst(210.0, (s * 0.025) as usize, s);
    let hat = noise_burst((s * 0.008) as usize, s);

    let mut buf = vec![0.0f32; total];
    let mut pos = 0;
    let mut beat_in_bar = 0;
    while pos < total {
        // Kick on 1 only
        if beat_in_bar == 0 {
            place(&mut buf, pos, &kick, 0.8);
        }
        // Snare on 2 (second beat = 2-step)
        if beat_in_bar == 1 {
            place(&mut buf, pos, &snare, 0.7);
        }
        // 8th note hats
        place(&mut buf, pos, &hat, 0.35);
        if pos + eighth < total {
            place(&mut buf, pos + eighth, &hat, 0.3);
        }
        pos += beat;
        beat_in_bar = (beat_in_bar + 1) % 2; // 2-beat pattern
    }
    buf
}

// ============================================================================
// Tolerance
// ============================================================================

const TOL: f64 = 2.0; // BPM tolerance for complex patterns
const TOL_HARD: f64 = 4.0; // wider for the hardest patterns

// ============================================================================
// Tests: Standard 4/4 across BPM range
// ============================================================================

#[test]
fn standard_4_4_house() {
    for &bpm in &[120.0, 124.0, 126.0, 128.0, 130.0] {
        let samples = standard_4_4(bpm, 44100, 15.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - bpm).abs() < TOL,
            "4/4 house at {} BPM: got {}", bpm, result.bpm
        );
    }
}

#[test]
fn standard_4_4_techno() {
    for &bpm in &[132.0, 135.0, 138.0, 140.0, 145.0] {
        let samples = standard_4_4(bpm, 44100, 15.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - bpm).abs() < TOL,
            "4/4 techno at {} BPM: got {}", bpm, result.bpm
        );
    }
}

#[test]
fn standard_4_4_dnb() {
    for &bpm in &[170.0, 174.0, 176.0] {
        let samples = standard_4_4(bpm, 44100, 15.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - bpm).abs() < TOL,
            "4/4 DnB at {} BPM: got {}", bpm, result.bpm
        );
    }
}

// ============================================================================
// Tests: Triplet hi-hat pattern (the 4/3 error)
// ============================================================================

#[test]
fn triplet_hihat_126bpm() {
    let samples = triplet_hihat_pattern(126.0, 44100, 20.0);
    let result = detect(&samples, 44100);
    // Must detect 126 BPM (quarter note), NOT 168 (126 * 4/3)
    assert!(
        (result.bpm - 126.0).abs() < TOL_HARD,
        "Triplet hat at 126 BPM: got {} (4/3 would be 168)", result.bpm
    );
}

#[test]
fn triplet_hihat_140bpm() {
    let samples = triplet_hihat_pattern(140.0, 44100, 20.0);
    let result = detect(&samples, 44100);
    assert!(
        (result.bpm - 140.0).abs() < TOL_HARD,
        "Triplet hat at 140 BPM: got {} (4/3 would be 186.7)", result.bpm
    );
}

#[test]
fn triplet_hihat_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[100.0, 110.0, 120.0, 126.0, 130.0, 140.0, 148.0] {
        let samples = triplet_hihat_pattern(bpm, 44100, 20.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "Triplet hi-hat failures: {:?}", failures
    );
}

// ============================================================================
// Tests: Half-time groove (the 0.5x error)
// ============================================================================

#[test]
fn halftime_140bpm() {
    let samples = halftime_pattern(140.0, 44100, 20.0);
    let result = detect(&samples, 44100);
    assert!(
        (result.bpm - 140.0).abs() < TOL_HARD,
        "Half-time at 140 BPM: got {} (half would be 70)", result.bpm
    );
}

#[test]
fn halftime_174bpm() {
    let samples = halftime_pattern(174.0, 44100, 20.0);
    let result = detect(&samples, 44100);
    assert!(
        (result.bpm - 174.0).abs() < TOL_HARD,
        "Half-time at 174 BPM: got {} (half would be 87)", result.bpm
    );
}

#[test]
fn halftime_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[128.0, 135.0, 140.0, 150.0, 160.0, 170.0, 174.0] {
        let samples = halftime_pattern(bpm, 44100, 20.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "Half-time failures: {:?}", failures
    );
}

// ============================================================================
// Tests: Offbeat hi-hat (the 2x error)
// ============================================================================

#[test]
fn offbeat_hat_128bpm() {
    let samples = offbeat_hihat_pattern(128.0, 44100, 15.0);
    let result = detect(&samples, 44100);
    assert!(
        (result.bpm - 128.0).abs() < TOL_HARD,
        "Offbeat hat at 128 BPM: got {} (2x would be 256)", result.bpm
    );
}

#[test]
fn offbeat_hat_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[120.0, 125.0, 128.0, 130.0, 135.0, 140.0] {
        let samples = offbeat_hihat_pattern(bpm, 44100, 15.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "Offbeat hat failures: {:?}", failures
    );
}

// ============================================================================
// Tests: Sparse kick + triplet hat (the 2/3 error)
// ============================================================================

#[test]
fn sparse_triplet_128bpm() {
    let samples = sparse_kick_triplet_hat(128.0, 44100, 20.0);
    let result = detect(&samples, 44100);
    assert!(
        (result.bpm - 128.0).abs() < TOL_HARD,
        "Sparse kick + triplet hat at 128 BPM: got {} (2/3 would be 85.3)", result.bpm
    );
}

#[test]
fn sparse_triplet_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[120.0, 128.0, 140.0, 150.0, 170.0] {
        let samples = sparse_kick_triplet_hat(bpm, 44100, 20.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "Sparse+triplet failures: {:?}", failures
    );
}

// ============================================================================
// Tests: Syncopated techno
// ============================================================================

#[test]
fn syncopated_techno_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[128.0, 133.0, 138.0, 140.0, 145.0] {
        let samples = syncopated_techno(bpm, 44100, 15.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "Syncopated techno failures: {:?}", failures
    );
}

// ============================================================================
// Tests: Minimal techno (low energy)
// ============================================================================

#[test]
fn minimal_techno_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[120.0, 125.0, 130.0, 135.0] {
        let samples = minimal_techno(bpm, 44100, 20.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "Minimal techno failures: {:?}", failures
    );
}

// ============================================================================
// Tests: DnB two-step (high tempo octave stability)
// ============================================================================

#[test]
fn dnb_twostep_range() {
    let mut failures: Vec<(f64, f64)> = Vec::new();
    for &bpm in &[170.0, 174.0, 176.0, 180.0] {
        let samples = dnb_twostep(bpm, 44100, 15.0);
        let result = detect(&samples, 44100);
        if (result.bpm - bpm).abs() >= TOL_HARD {
            failures.push((bpm, result.bpm));
        }
    }
    assert!(
        failures.is_empty(),
        "DnB two-step failures: {:?}", failures
    );
}
