//! Beat tracking: grid alignment, fine refinement, PLL grid offset, integer snapping.

use crate::onset::Onset;

/// Grid alignment tolerance as fraction of beat period.
const GRID_TOLERANCE: f64 = 0.12;
/// Fine refinement sweep range in BPM.
const REFINE_RANGE: f64 = 2.5;
/// Fine refinement step in BPM (coarse pass).
const REFINE_STEP: f64 = 0.1;
/// Second-pass refinement step (fine).
const REFINE_STEP_FINE: f64 = 0.01;
/// Max onsets to consider for grid alignment.
const MAX_ONSETS_FOR_GRID: usize = 80;
/// Max phase candidates to test.
const MAX_PHASE_CANDIDATES: usize = 16;
/// Integer snap threshold in BPM.
/// Just above the fine refinement step (0.01) so that the
/// nearest refinement candidate always qualifies for snap.
const SNAP_THRESHOLD: f64 = 0.02;
/// Minimum grid score ratio for integer snap.
const SNAP_SCORE_RATIO: f64 = 0.95;
/// Number of PLL phase offsets to test.
const PLL_RESOLUTION: usize = 100;

/// Compute grid alignment score for a given BPM and phase.
///
/// Tests how well the detected onsets align to a regular grid at the given BPM,
/// using Gaussian-weighted distance scoring.
pub fn grid_alignment_score(onsets: &[Onset], bpm: f64, phase: f64, _sample_rate: f64) -> f64 {
    if onsets.is_empty() || bpm <= 0.0 {
        return 0.0;
    }

    let beat_period = 60.0 / bpm;
    let tolerance = beat_period * GRID_TOLERANCE;
    let max_onsets = onsets.len().min(MAX_ONSETS_FOR_GRID);

    let mut score = 0.0;
    let mut weight_sum = 0.0;

    for onset in &onsets[..max_onsets] {
        let delta = (onset.time - phase).abs();
        let beat_fraction = (delta / beat_period) % 1.0;
        let distance_to_grid = beat_fraction.min(1.0 - beat_fraction) * beat_period;

        if distance_to_grid < tolerance {
            // Fast parabolic approximation of Gaussian: (1 - x^2)^2
            let x = distance_to_grid / tolerance;
            let gaussian_weight = (1.0 - x * x) * (1.0 - x * x);
            score += onset.strength * gaussian_weight;
        }

        weight_sum += onset.strength;
    }

    if weight_sum > 0.0 {
        score / weight_sum
    } else {
        0.0
    }
}

/// Find the best grid alignment score across multiple phase candidates.
fn best_grid_score(onsets: &[Onset], bpm: f64, sample_rate: f64) -> f64 {
    if onsets.is_empty() || bpm <= 0.0 {
        return 0.0;
    }

    let n_candidates = onsets.len().min(MAX_PHASE_CANDIDATES);
    let mut best_score = 0.0;

    for i in 0..n_candidates {
        let phase = onsets[i].time;
        let score = grid_alignment_score(onsets, bpm, phase, sample_rate);
        if score > best_score {
            best_score = score;
        }
    }

    best_score
}

/// Fine-refine BPM by sweeping ±REFINE_RANGE at REFINE_STEP resolution.
///
/// Returns (refined_bpm, best_grid_score).
pub fn refine_bpm(
    onsets: &[Onset],
    initial_bpm: f64,
    sample_rate: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> (f64, f64) {
    if onsets.is_empty() || initial_bpm <= 0.0 {
        return (initial_bpm, 0.0);
    }

    let mut best_bpm = initial_bpm;
    let mut best_score = best_grid_score(onsets, initial_bpm, sample_rate);

    // Coarse pass: ±2.5 BPM at 0.1 step
    let lo = (initial_bpm - REFINE_RANGE).max(min_bpm);
    let hi = (initial_bpm + REFINE_RANGE).min(max_bpm);

    let mut candidate = lo;
    while candidate <= hi {
        let score = best_grid_score(onsets, candidate, sample_rate);
        if score > best_score {
            best_score = score;
            best_bpm = candidate;
        }
        candidate += REFINE_STEP;
    }

    // Fine pass: ±0.1 BPM at 0.01 step around coarse winner
    let fine_lo = (best_bpm - REFINE_STEP).max(min_bpm);
    let fine_hi = (best_bpm + REFINE_STEP).min(max_bpm);
    candidate = fine_lo;
    while candidate <= fine_hi {
        let score = best_grid_score(onsets, candidate, sample_rate);
        if score > best_score {
            best_score = score;
            best_bpm = candidate;
        }
        candidate += REFINE_STEP_FINE;
    }

    (best_bpm, best_score)
}

/// Find grid offset (first beat time) using Phase-Locked Loop approach.
///
/// Tests many phase offsets over one beat period and picks the phase
/// that maximizes accumulated onset energy at beat positions.
pub fn find_grid_offset(onsets: &[Onset], bpm: f64, _sample_rate: f64) -> f64 {
    if onsets.is_empty() || bpm <= 0.0 {
        return 0.0;
    }

    let beat_period = 60.0 / bpm;

    // Use first onset as reference, then test offsets within one beat period
    let first_time = onsets[0].time;
    let mut best_offset = first_time;
    let mut best_energy = 0.0f64;

    for step in 0..PLL_RESOLUTION {
        let phase = first_time + (step as f64 / PLL_RESOLUTION as f64) * beat_period;

        // Accumulate onset energy at beat positions
        let mut energy = 0.0;
        for onset in onsets {
            let delta = (onset.time - phase).abs();
            let beat_fraction = (delta / beat_period) % 1.0;
            let distance = beat_fraction.min(1.0 - beat_fraction) * beat_period;
            let tolerance = beat_period * GRID_TOLERANCE;

            if distance < tolerance {
                let x = distance / tolerance;
                energy += onset.strength * (1.0 - x * x);
            }
        }

        if energy > best_energy {
            best_energy = energy;
            best_offset = phase;
        }
    }

    // Normalize offset to be within [0, beat_period) from the start of the audio
    let offset = best_offset % beat_period;
    if offset < 0.0 {
        offset + beat_period
    } else {
        offset
    }
}

/// Snap BPM to nearest integer if close enough and grid score doesn't degrade.
pub fn snap_to_integer(bpm: f64, onsets: &[Onset], sample_rate: f64) -> f64 {
    let rounded = bpm.round();
    if (bpm - rounded).abs() > SNAP_THRESHOLD {
        return bpm;
    }

    let original_score = best_grid_score(onsets, bpm, sample_rate);
    let rounded_score = best_grid_score(onsets, rounded, sample_rate);

    if original_score < 1e-10 || rounded_score >= original_score * SNAP_SCORE_RATIO {
        rounded
    } else {
        bpm
    }
}

/// Wide snap for machine-timed tracks: snaps within 0.5 BPM (mixi-style).
/// Only used when the Bouncer confirms machine timing (jitter < 2ms).
pub fn snap_to_integer_wide(bpm: f64, onsets: &[Onset], sample_rate: f64) -> f64 {
    let rounded = bpm.round();
    if (bpm - rounded).abs() > 0.5 {
        return bpm;
    }

    let original_score = best_grid_score(onsets, bpm, sample_rate);
    let rounded_score = best_grid_score(onsets, rounded, sample_rate);

    if original_score < 1e-10 || rounded_score >= original_score * SNAP_SCORE_RATIO {
        rounded
    } else {
        bpm
    }
}

/// Compute a "bar integrality" score for a BPM given a track duration.
///
/// In produced EDM, the total number of bars (duration * bpm / 240) is almost
/// always an integer (or very close). This score measures how close the bar
/// count is to the nearest integer — a BPM that produces 64.0 bars scores
/// higher than one producing 63.7 bars.
///
/// Returns a score in [0, 1] where 1 = perfectly integer bars.
pub fn bar_count_score(bpm: f64, duration_secs: f64) -> f64 {
    if bpm <= 0.0 || duration_secs <= 0.0 {
        return 0.0;
    }

    let bars = duration_secs * bpm / 240.0; // 4 beats per bar

    // Distance from nearest half-integer (tracks are often cut at half-phrase)
    // e.g. 61.5 bars is "clean" (half-phrase boundary), 61.3 is not
    let half_bars = bars * 2.0; // convert to half-bars
    let frac = (half_bars - half_bars.round()).abs();

    // Score: 1.0 at frac=0, drops to 0 at frac=0.5
    (1.0 - frac * 2.0).max(0.0)
}

/// Test multiple metrical variants of a BPM and return the one with the
/// best bar integrality. Only overrides if the alternative is clearly better.
///
/// Factors tested: 1, 2, 1/2, 4/3, 3/4
pub fn bar_count_resolve(bpm: f64, duration_secs: f64, min_bpm: f64, max_bpm: f64) -> f64 {
    let factors: &[f64] = &[1.0, 2.0, 0.5, 4.0 / 3.0, 3.0 / 4.0];

    let original_score = bar_count_score(bpm, duration_secs);
    let mut best_bpm = bpm;
    let mut best_score = original_score;

    for &f in factors {
        let candidate = bpm * f;
        if candidate < min_bpm || candidate > max_bpm {
            continue;
        }
        let score = bar_count_score(candidate, duration_secs);

        // Only override if:
        // The alternative is near-perfect (>0.95) and original is poor (<0.5).
        // This is very conservative — only fires when the bar count evidence
        // is overwhelming. Avoids false corrections on short/synthetic signals.
        let should_override = score > 0.95 && best_score < 0.5;

        if should_override {
            best_score = score;
            best_bpm = candidate;
        }
    }

    best_bpm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::{Band, Onset};

    fn make_grid_onsets(bpm: f64, offset: f64, n_beats: usize) -> Vec<Onset> {
        let period = 60.0 / bpm;
        (0..n_beats)
            .map(|i| Onset {
                time: offset + i as f64 * period,
                strength: 1.0,
                band: Band::Low,
            })
            .collect()
    }

    #[test]
    fn test_grid_alignment_perfect() {
        let onsets = make_grid_onsets(120.0, 0.1, 20);
        let score = grid_alignment_score(&onsets, 120.0, 0.1, 44100.0);
        assert!(score > 0.9, "Perfect grid should score > 0.9, got {}", score);
    }

    #[test]
    fn test_grid_alignment_wrong_bpm() {
        let onsets = make_grid_onsets(120.0, 0.1, 20);
        let score_correct = grid_alignment_score(&onsets, 120.0, 0.1, 44100.0);
        let score_wrong = grid_alignment_score(&onsets, 133.0, 0.1, 44100.0);
        assert!(
            score_correct > score_wrong,
            "Correct BPM should score higher"
        );
    }

    #[test]
    fn test_refine_bpm() {
        let onsets = make_grid_onsets(128.0, 0.0, 30);
        let (refined, score) = refine_bpm(&onsets, 127.0, 44100.0, 60.0, 200.0);
        assert!(
            (refined - 128.0).abs() < 0.5,
            "Should refine to ~128, got {}",
            refined
        );
        assert!(score > 0.5, "Should have decent grid score");
    }

    #[test]
    fn test_find_grid_offset() {
        let offset = 0.15;
        let onsets = make_grid_onsets(120.0, offset, 20);
        let detected = find_grid_offset(&onsets, 120.0, 44100.0);
        assert!(
            (detected - offset).abs() < 0.02,
            "Offset should be ~{}, got {}",
            offset,
            detected
        );
    }

    #[test]
    fn test_snap_to_integer() {
        let onsets = make_grid_onsets(120.0, 0.0, 30);
        // 120.005 is within 0.01 threshold
        let snapped = snap_to_integer(120.005, &onsets, 44100.0);
        assert_eq!(snapped, 120.0, "Should snap 120.005 to 120");
    }

    #[test]
    fn test_no_snap_when_far() {
        let onsets = make_grid_onsets(120.0, 0.0, 30);
        // 120.1 is beyond 0.01 threshold
        let snapped = snap_to_integer(120.1, &onsets, 44100.0);
        assert!(
            (snapped - 120.1).abs() < 0.01,
            "Should NOT snap when far from integer"
        );
    }
}
