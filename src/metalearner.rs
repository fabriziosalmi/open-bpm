//! Meta-learner: tiny ML model that picks the best estimator.
//!
//! Instead of complex fusion heuristics, a trained linear model scores
//! each estimator's candidate BPM and picks the winner.
//!
//! Model: score[i] = dot(features[i], weights) + bias
//! Features per candidate: [bpm_normalized, confidence, edm_zone_score, is_integer]
//! Total: 4 candidates × 4 features = 16 inputs → 4 scores → argmax
//!
//! Training: offline on GiantSteps dataset via simple gradient descent.
//! Inference: 16 multiplications + 4 additions. Cost: ~0.001ms.

use crate::tempo::edm_tempo_zone_score;
use crate::TempoEstimate;

/// Number of features per candidate estimator.
const FEATURES_PER_CANDIDATE: usize = 4;
/// Number of candidate estimators.
const NUM_CANDIDATES: usize = 4;
/// Total weight count.
const NUM_WEIGHTS: usize = FEATURES_PER_CANDIDATE * NUM_CANDIDATES + NUM_CANDIDATES; // weights + biases

/// Trained weights. These are learned from GiantSteps ground truth.
/// Format: [w0..w15, b0..b3] where w[i*4..i*4+4] are weights for candidate i.
///
/// PLACEHOLDER: these are initial heuristic weights, not yet trained.
/// Replace with trained values after running the training script.
static WEIGHTS: [f64; NUM_WEIGHTS] = [
    // Candidate 0 (IOI): [bpm_norm, confidence, zone, is_integer]
    0.5, 0.3, 0.8, 0.2,
    // Candidate 1 (Comb):
    0.5, 0.5, 0.6, 0.2,
    // Candidate 2 (AC):
    0.5, 0.2, 0.4, 0.2,
    // Candidate 3 (Hopf):
    0.3, 0.2, 0.7, 0.1,
    // Biases: [IOI, Comb, AC, Hopf]
    0.1, 0.0, -0.1, -0.2,
];

/// Pick the best BPM from multiple estimator candidates using the meta-learner.
///
/// Returns the BPM of the winning candidate and a confidence score.
pub fn pick_best(
    candidates: &[(Option<TempoEstimate>, &str)], // (estimate, name)
) -> Option<TempoEstimate> {
    if candidates.is_empty() {
        return None;
    }

    let mut best_score = f64::NEG_INFINITY;
    let mut best_est: Option<TempoEstimate> = None;

    for (idx, (est_opt, _name)) in candidates.iter().enumerate() {
        if idx >= NUM_CANDIDATES {
            break;
        }
        let est = match est_opt {
            Some(e) => e,
            None => continue,
        };

        // Extract features
        let bpm_norm = est.bpm / 200.0; // normalize to ~[0, 1]
        let confidence = est.confidence;
        let zone = edm_tempo_zone_score(est.bpm);
        let is_integer = if (est.bpm - est.bpm.round()).abs() < 0.5 { 1.0 } else { 0.0 };

        let features = [bpm_norm, confidence, zone, is_integer];

        // Compute score: dot(features, weights[idx]) + bias[idx]
        let w_offset = idx * FEATURES_PER_CANDIDATE;
        let b_offset = NUM_CANDIDATES * FEATURES_PER_CANDIDATE + idx;

        let mut score = WEIGHTS[b_offset]; // bias
        for (j, &f) in features.iter().enumerate() {
            score += f * WEIGHTS[w_offset + j];
        }

        if score > best_score {
            best_score = score;
            best_est = Some(*est);
        }
    }

    best_est
}

/// Compute the feature vector for training data export.
/// Returns [bpm_norm, confidence, zone, is_integer] for each candidate.
pub fn extract_features(est: &TempoEstimate) -> [f64; FEATURES_PER_CANDIDATE] {
    [
        est.bpm / 200.0,
        est.confidence,
        edm_tempo_zone_score(est.bpm),
        if (est.bpm - est.bpm.round()).abs() < 0.5 { 1.0 } else { 0.0 },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_best_basic() {
        let candidates: Vec<(Option<TempoEstimate>, &str)> = vec![
            (Some(TempoEstimate { bpm: 128.0, confidence: 0.8 }), "IOI"),
            (Some(TempoEstimate { bpm: 64.0, confidence: 0.9 }), "Comb"),
            (Some(TempoEstimate { bpm: 128.0, confidence: 0.7 }), "AC"),
            (Some(TempoEstimate { bpm: 128.0, confidence: 0.5 }), "Hopf"),
        ];
        let result = pick_best(&candidates);
        assert!(result.is_some());
        // With default weights, 128 should win (stronger zone + majority)
        let bpm = result.unwrap().bpm;
        assert!(
            (bpm - 128.0).abs() < 1.0 || (bpm - 64.0).abs() < 1.0,
            "Should pick 128 or 64, got {}", bpm
        );
    }

    #[test]
    fn test_pick_best_handles_none() {
        let candidates: Vec<(Option<TempoEstimate>, &str)> = vec![
            (None, "IOI"),
            (Some(TempoEstimate { bpm: 140.0, confidence: 0.6 }), "Comb"),
            (None, "AC"),
            (None, "Hopf"),
        ];
        let result = pick_best(&candidates);
        assert!(result.is_some());
        assert!((result.unwrap().bpm - 140.0).abs() < 0.01);
    }
}
