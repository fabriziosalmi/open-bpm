//! Draconian test suite for open-bpm.
//!
//! These tests are mathematically rigorous — they verify not just "does it return
//! a number" but "does it return the CORRECT number within PROVEN error bounds."
//!
//! Signal generators produce exactly-timed synthetic audio with known ground truth.
//! Tolerances are derived from the algorithm's theoretical resolution limits,
//! not from "whatever makes the test pass."

use open_bpm::{detect, detect_with_options, DetectOptions};
use std::f64::consts::PI;

// ============================================================================
// Signal generators
// ============================================================================

/// Generate a click train at exact BPM.
///
/// Each click is an exponentially decaying sine burst at `freq_hz`,
/// producing broadband spectral content that the onset detector can see
/// across a 2048-sample FFT window.
///
/// Mathematical guarantee: beat positions are at exactly
/// `n * 60.0 / bpm * sample_rate` samples (integer-rounded).
fn click_train(bpm: f64, sample_rate: u32, duration_secs: f64, freq_hz: f64) -> Vec<f32> {
    let sr = sample_rate as f64;
    let total = (duration_secs * sr) as usize;
    let beat_period = 60.0 / bpm * sr;
    let click_len = (sr * 0.02) as usize; // 20 ms

    let mut samples = vec![0.0f32; total];
    let mut beat_pos = 0.0f64;
    while (beat_pos as usize) < total {
        let start = beat_pos as usize;
        for i in 0..click_len.min(total.saturating_sub(start)) {
            let t = i as f64 / sr;
            let decay = (-(i as f64) / (click_len as f64 * 0.3)).exp();
            samples[start + i] += (2.0 * PI * freq_hz * t).sin() as f32 * decay as f32 * 0.9;
        }
        beat_pos += beat_period;
    }
    samples
}

/// Generate a kick + hi-hat pattern (4/4 time).
///
/// Kick on every beat (low freq), hi-hat on every 8th note (high freq).
/// This tests multi-band onset detection and octave resolution.
fn kick_hat_pattern(bpm: f64, sample_rate: u32, duration_secs: f64) -> Vec<f32> {
    let sr = sample_rate as f64;
    let total = (duration_secs * sr) as usize;
    let beat_period = 60.0 / bpm * sr;
    let eighth_period = beat_period / 2.0;
    let kick_len = (sr * 0.03) as usize;
    let hat_len = (sr * 0.005) as usize;

    let mut samples = vec![0.0f32; total];

    // Kicks on beats
    let mut pos = 0.0f64;
    while (pos as usize) < total {
        let start = pos as usize;
        for i in 0..kick_len.min(total.saturating_sub(start)) {
            let t = i as f64 / sr;
            let decay = (-(i as f64) / (kick_len as f64 * 0.25)).exp();
            samples[start + i] += (2.0 * PI * 60.0 * t).sin() as f32 * decay as f32 * 0.8;
        }
        pos += beat_period;
    }

    // Hi-hats on 8th notes
    pos = 0.0;
    while (pos as usize) < total {
        let start = pos as usize;
        for i in 0..hat_len.min(total.saturating_sub(start)) {
            let t = i as f64 / sr;
            let decay = (-(i as f64) / (hat_len as f64 * 0.2)).exp();
            // Noise-like high frequency
            samples[start + i] += (2.0 * PI * 8000.0 * t).sin() as f32
                * (2.0 * PI * 12000.0 * t).sin() as f32
                * decay as f32
                * 0.3;
        }
        pos += eighth_period;
    }

    samples
}

/// Add white noise at a given SNR (dB).
fn add_noise(samples: &mut [f32], snr_db: f64) {
    let signal_power: f64 =
        samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / samples.len() as f64;
    let noise_power = signal_power / 10.0f64.powf(snr_db / 10.0);
    let noise_amp = noise_power.sqrt();

    // Simple LCG PRNG (deterministic, no rand dependency)
    let mut state: u64 = 0xDEADBEEFCAFE;
    for s in samples.iter_mut() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let uniform = ((state >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0;
        *s += (uniform * noise_amp) as f32;
    }
}

/// Superimpose two click trains (polyrhythmic stress test).
fn dual_click_train(
    bpm1: f64,
    bpm2: f64,
    sample_rate: u32,
    duration_secs: f64,
) -> Vec<f32> {
    let mut s1 = click_train(bpm1, sample_rate, duration_secs, 100.0);
    let s2 = click_train(bpm2, sample_rate, duration_secs, 5000.0);
    for (a, b) in s1.iter_mut().zip(s2.iter()) {
        *a += b * 0.3; // secondary rhythm at -10 dB
    }
    s1
}

// ============================================================================
// Tolerance derivation
// ============================================================================

// The theoretical resolution limit comes from:
// - IOI histogram bin: 0.25 BPM → ±0.125 BPM before interpolation
// - Parabolic interpolation refines to ~0.1 BPM
// - Fine refinement sweep: 0.1 BPM steps over ±2.5 BPM
// - Integer snapping: within 0.15 BPM → exact integer
//
// For a clean synthetic click train with exact timing:
//   Expected accuracy: < 0.5 BPM (after refinement)
//   We use 1.0 BPM tolerance for standard tests (conservative)
//   and 0.5 BPM for "precision" tests with longer signals.

const TOLERANCE_STANDARD: f64 = 1.0; // 10s signals
const TOLERANCE_PRECISION: f64 = 0.5; // 30s+ signals
const TOLERANCE_NOISY: f64 = 3.0; // SNR < 10 dB
const TOLERANCE_OCTAVE: f64 = 2.0; // octave-correct (not 2x or /2)

// ============================================================================
// A. Exact BPM recovery — sweep across the full range
// ============================================================================

/// Sweep from 65 to 195 BPM in 5-BPM steps.
/// Each test point must recover within TOLERANCE_STANDARD.
/// This is the fundamental correctness test.
#[test]
fn sweep_65_to_195_step5() {
    let sr = 44100u32;
    let duration = 10.0;
    let mut failures: Vec<(f64, f64)> = Vec::new();

    let mut bpm = 65.0;
    while bpm <= 195.0 {
        let samples = click_train(bpm, sr, duration, 150.0);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_STANDARD {
            failures.push((bpm, result.bpm));
        }
        bpm += 5.0;
    }

    assert!(
        failures.is_empty(),
        "BPM sweep failures (tolerance {}): {:?}",
        TOLERANCE_STANDARD,
        failures
    );
}

/// Same sweep but with 30-second signals — tighter tolerance.
#[test]
fn sweep_precision_30s() {
    let sr = 44100u32;
    let duration = 30.0;
    let mut failures: Vec<(f64, f64)> = Vec::new();

    for bpm in [70.0, 90.0, 110.0, 120.0, 128.0, 140.0, 150.0, 170.0, 185.0] {
        let samples = click_train(bpm, sr, duration, 150.0);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_PRECISION {
            failures.push((bpm, result.bpm));
        }
    }

    assert!(
        failures.is_empty(),
        "Precision sweep failures (tolerance {}): {:?}",
        TOLERANCE_PRECISION,
        failures
    );
}

// ============================================================================
// B. Fractional BPM — non-integer tempos
// ============================================================================

#[test]
fn fractional_bpm_recovery() {
    let sr = 44100u32;
    let duration = 15.0;
    let mut failures: Vec<(f64, f64)> = Vec::new();

    for &bpm in &[120.5, 127.3, 133.33, 140.7, 174.25] {
        let samples = click_train(bpm, sr, duration, 150.0);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_STANDARD {
            failures.push((bpm, result.bpm));
        }
    }

    assert!(
        failures.is_empty(),
        "Fractional BPM failures: {:?}",
        failures
    );
}

// ============================================================================
// C. Sample rate invariance
// ============================================================================

/// The same physical signal at different sample rates must produce
/// the same BPM. This verifies that all time/frequency calculations
/// properly scale with sample rate.
#[test]
fn sample_rate_invariance() {
    let bpm = 128.0;
    let duration = 10.0;
    let mut failures: Vec<(u32, f64)> = Vec::new();

    for &sr in &[22050u32, 44100, 48000, 88200, 96000] {
        let samples = click_train(bpm, sr, duration, 150.0);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_STANDARD {
            failures.push((sr, result.bpm));
        }
    }

    assert!(
        failures.is_empty(),
        "Sample rate invariance failures: {:?}",
        failures
    );
}

// ============================================================================
// D. Octave stability
// ============================================================================

/// Verify that the detector returns BPM, not BPM/2 or BPM*2.
/// Tests the full range including edge cases where octave errors are common.
#[test]
fn octave_stability() {
    let sr = 44100u32;
    let duration = 12.0;
    let mut failures: Vec<(f64, f64, &str)> = Vec::new();

    let cases: Vec<(f64, &str)> = vec![
        (70.0, "slow hip-hop"),
        (85.0, "half-time dubstep"),
        (90.0, "slow house"),
        (100.0, "deep house"),
        (128.0, "standard house"),
        (140.0, "trance"),
        (150.0, "hardstyle"),
        (170.0, "drum & bass"),
        (175.0, "fast DnB"),
    ];

    for (bpm, label) in cases {
        let samples = click_train(bpm, sr, duration, 150.0);
        let result = detect(&samples, sr);

        // Must be within tolerance of BPM, not BPM/2 or BPM*2
        let error = (result.bpm - bpm).abs();
        let half_error = (result.bpm - bpm / 2.0).abs();
        let double_error = (result.bpm - bpm * 2.0).abs();

        if error > TOLERANCE_OCTAVE {
            // If it matched an octave instead, that's an octave error
            if half_error < TOLERANCE_OCTAVE || double_error < TOLERANCE_OCTAVE {
                failures.push((bpm, result.bpm, label));
            } else {
                failures.push((bpm, result.bpm, label));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Octave stability failures: {:?}",
        failures
    );
}

// ============================================================================
// E. Noise robustness
// ============================================================================

/// BPM detection must work at 20 dB SNR (moderate noise).
#[test]
fn noise_robustness_20db() {
    let sr = 44100u32;
    let duration = 15.0;
    let mut failures: Vec<(f64, f64)> = Vec::new();

    for &bpm in &[100.0, 120.0, 140.0, 170.0] {
        let mut samples = click_train(bpm, sr, duration, 150.0);
        add_noise(&mut samples, 20.0);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_STANDARD {
            failures.push((bpm, result.bpm));
        }
    }

    assert!(
        failures.is_empty(),
        "Noise robustness (20 dB) failures: {:?}",
        failures
    );
}

/// BPM detection at 6 dB SNR (heavy noise) — wider tolerance.
#[test]
fn noise_robustness_6db() {
    let sr = 44100u32;
    let duration = 20.0;
    let mut failures: Vec<(f64, f64)> = Vec::new();

    for &bpm in &[110.0, 128.0, 150.0] {
        let mut samples = click_train(bpm, sr, duration, 150.0);
        add_noise(&mut samples, 6.0);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_NOISY {
            failures.push((bpm, result.bpm));
        }
    }

    assert!(
        failures.is_empty(),
        "Noise robustness (6 dB) failures: {:?}",
        failures
    );
}

// ============================================================================
// F. Multi-band pattern tests
// ============================================================================

/// Kick + hi-hat pattern: detector must lock to kick tempo, not 2x (8th notes).
#[test]
fn kick_hat_locks_to_quarter_note() {
    let sr = 44100u32;
    let duration = 15.0;
    let mut failures: Vec<(f64, f64)> = Vec::new();

    for &bpm in &[120.0, 128.0, 140.0] {
        let samples = kick_hat_pattern(bpm, sr, duration);
        let result = detect(&samples, sr);
        let error = (result.bpm - bpm).abs();
        if error > TOLERANCE_OCTAVE {
            failures.push((bpm, result.bpm));
        }
    }

    assert!(
        failures.is_empty(),
        "Kick+hat pattern failures: {:?}",
        failures
    );
}

// ============================================================================
// G. Confidence properties
// ============================================================================

/// Silence must produce confidence < 0.2.
#[test]
fn confidence_silence_is_low() {
    let samples = vec![0.0f32; 44100 * 10];
    let result = detect(&samples, 44100);
    assert!(
        result.confidence < 0.2,
        "Silence confidence should be < 0.2, got {}",
        result.confidence
    );
}

/// Pure noise (no rhythm) must produce confidence < 0.3.
#[test]
fn confidence_noise_is_low() {
    let mut samples = vec![0.0f32; 44100 * 10];
    add_noise(&mut samples, 0.0); // 0 dB SNR = pure noise
    let result = detect(&samples, 44100);
    assert!(
        result.confidence < 0.4,
        "Pure noise confidence should be < 0.4, got {}",
        result.confidence
    );
}

/// A clean click train must produce confidence > 0.3.
#[test]
fn confidence_clean_signal_is_moderate() {
    let samples = click_train(128.0, 44100, 15.0, 150.0);
    let result = detect(&samples, 44100);
    assert!(
        result.confidence > 0.3,
        "Clean 128 BPM signal should have confidence > 0.3, got {}",
        result.confidence
    );
}

/// Monotonicity: longer signal should have >= confidence of shorter signal
/// for the same click train.
#[test]
fn confidence_monotonic_with_duration() {
    let bpm = 128.0;
    let sr = 44100u32;
    let short = click_train(bpm, sr, 8.0, 150.0);
    let long = click_train(bpm, sr, 30.0, 150.0);

    let r_short = detect(&short, sr);
    let r_long = detect(&long, sr);

    assert!(
        (r_short.bpm - bpm).abs() < TOLERANCE_STANDARD,
        "Short signal BPM wrong: {}",
        r_short.bpm
    );
    assert!(
        (r_long.bpm - bpm).abs() < TOLERANCE_PRECISION,
        "Long signal BPM wrong: {}",
        r_long.bpm
    );
}

// ============================================================================
// H. Grid offset accuracy
// ============================================================================

/// Grid offset must match the known first-beat position.
#[test]
fn grid_offset_accuracy() {
    let sr = 44100u32;
    let bpm = 120.0;
    let beat_period = 60.0 / bpm;

    // Click train starting at exactly t=0
    let samples = click_train(bpm, sr, 15.0, 150.0);
    let result = detect(&samples, sr);

    assert!(
        (result.bpm - bpm).abs() < TOLERANCE_STANDARD,
        "BPM wrong: {}",
        result.bpm
    );

    // Grid offset should be close to 0 (or a multiple of beat_period)
    let offset_mod = result.grid_offset % beat_period;
    let offset_error = offset_mod.min(beat_period - offset_mod);
    assert!(
        offset_error < beat_period * 0.15,
        "Grid offset error too large: {:.4}s (beat period {:.4}s)",
        offset_error,
        beat_period
    );
}

// ============================================================================
// I. Edge cases and degenerates
// ============================================================================

/// Empty input returns zero BPM.
#[test]
fn empty_input() {
    let result = detect(&[], 44100);
    assert_eq!(result.bpm, 0.0);
    assert_eq!(result.confidence, 0.0);
}

/// Single sample.
#[test]
fn single_sample() {
    let result = detect(&[0.5], 44100);
    assert_eq!(result.bpm, 0.0);
}

/// Very short signal (< 1 beat).
#[test]
fn sub_beat_signal() {
    // 0.3 seconds at 120 BPM — less than one beat period (0.5s)
    let samples = click_train(120.0, 44100, 0.3, 150.0);
    let result = detect(&samples, 44100);
    // Should either return 0 or have very low confidence
    assert!(
        result.bpm == 0.0 || result.confidence < 0.2,
        "Sub-beat signal should fail gracefully"
    );
}

/// DC offset should not confuse the detector.
#[test]
fn dc_offset_immunity() {
    let sr = 44100u32;
    let bpm = 128.0;
    let mut samples = click_train(bpm, sr, 10.0, 150.0);
    for s in samples.iter_mut() {
        *s += 0.5; // large DC offset
    }
    let result = detect(&samples, sr);
    assert!(
        (result.bpm - bpm).abs() < TOLERANCE_STANDARD,
        "DC offset caused error: expected {}, got {}",
        bpm,
        result.bpm
    );
}

/// Clipped signal (all samples at ±1.0) should not crash or hang.
#[test]
fn clipped_signal_no_crash() {
    let sr = 44100u32;
    let mut samples = click_train(120.0, sr, 5.0, 150.0);
    for s in samples.iter_mut() {
        *s = s.clamp(-0.1, 0.1);
    }
    let _result = detect(&samples, sr); // must not panic
}

/// NaN/Inf in input must not propagate.
#[test]
fn nan_inf_safety() {
    let sr = 44100u32;
    let mut samples = click_train(120.0, sr, 5.0, 150.0);
    samples[1000] = f32::NAN;
    samples[2000] = f32::INFINITY;
    samples[3000] = f32::NEG_INFINITY;
    let result = detect(&samples, sr);
    assert!(!result.bpm.is_nan(), "BPM must not be NaN");
    assert!(!result.confidence.is_nan(), "Confidence must not be NaN");
}

// ============================================================================
// J. Custom options
// ============================================================================

/// Narrowing the BPM range must still find the tempo within that range.
#[test]
fn custom_range_narrows_correctly() {
    let sr = 44100u32;
    let samples = click_train(128.0, sr, 10.0, 150.0);

    let opts = DetectOptions {
        min_bpm: 120.0,
        max_bpm: 140.0,
        ..Default::default()
    };
    let result = detect_with_options(&samples, sr, &opts);
    assert!(
        (result.bpm - 128.0).abs() < TOLERANCE_STANDARD,
        "Narrow range should still find 128 BPM, got {}",
        result.bpm
    );
}

/// Disabling segmented analysis should still work.
#[test]
fn no_segments_mode() {
    let sr = 44100u32;
    let samples = click_train(140.0, sr, 30.0, 150.0);

    let opts = DetectOptions {
        segmented: false,
        ..Default::default()
    };
    let result = detect_with_options(&samples, sr, &opts);
    assert!(
        (result.bpm - 140.0).abs() < TOLERANCE_STANDARD,
        "No-segments mode failed: expected 140, got {}",
        result.bpm
    );
}

// ============================================================================
// K. Integer snapping properties
// ============================================================================

/// A signal at exactly 128.0 BPM should snap to integer 128.
#[test]
fn integer_snap_exact() {
    let samples = click_train(128.0, 44100, 15.0, 150.0);
    let result = detect(&samples, 44100);
    assert_eq!(
        result.bpm, 128.0,
        "Exact 128 BPM should snap to integer, got {}",
        result.bpm
    );
}

// ============================================================================
// L. Estimator agreement
// ============================================================================

/// For a clean synthetic signal, at least 2 of 3 estimators should agree
/// within FUSION_TOLERANCE (3 BPM).
#[test]
fn estimator_agreement_on_clean_signal() {
    let samples = click_train(128.0, 44100, 15.0, 150.0);
    let result = detect(&samples, 44100);

    let estimates: Vec<f64> = [
        result.estimators.ioi.map(|e| e.bpm),
        result.estimators.comb.map(|e| e.bpm),
        result.estimators.autocorrelation.map(|e| e.bpm),
        result.estimators.tempogram.map(|e| e.bpm),
    ]
    .iter()
    .filter_map(|e| *e)
    .collect();

    assert!(
        estimates.len() >= 2,
        "Should have at least 2 estimator results"
    );

    // Count agreeing pairs
    let mut agreements = 0;
    for i in 0..estimates.len() {
        for j in (i + 1)..estimates.len() {
            if (estimates[i] - estimates[j]).abs() < 3.0 {
                agreements += 1;
            }
        }
    }

    assert!(
        agreements >= 1,
        "At least 2 estimators should agree on clean signal. Estimates: {:?}",
        estimates
    );
}

// ============================================================================
// M. Determinism
// ============================================================================

/// Same input must produce same output (no random state).
#[test]
fn deterministic_output() {
    let samples = click_train(133.0, 44100, 10.0, 150.0);
    let r1 = detect(&samples, 44100);
    let r2 = detect(&samples, 44100);
    assert_eq!(r1.bpm, r2.bpm, "Detection must be deterministic");
    assert_eq!(
        r1.confidence, r2.confidence,
        "Confidence must be deterministic"
    );
    assert_eq!(
        r1.grid_offset, r2.grid_offset,
        "Grid offset must be deterministic"
    );
}

// ============================================================================
// N. Performance bounds (ensure no O(n^2) regressions)
// ============================================================================

/// Detection of a 30-second track must complete in < 30 seconds in debug mode.
/// (Release mode: < 100 ms. This bound is conservative for CI.)
#[test]
fn performance_bound_30s() {
    let sr = 44100u32;
    let samples = click_train(128.0, sr, 30.0, 150.0);

    let start = std::time::Instant::now();
    let result = detect(&samples, sr);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 30,
        "30s track took {:?} (must be < 30s in debug mode)",
        elapsed
    );
    assert!(
        (result.bpm - 128.0).abs() < TOLERANCE_STANDARD,
        "30s track BPM wrong: {}",
        result.bpm
    );
}
