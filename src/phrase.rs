//! Phrase structure analysis: resolves half/double BPM ambiguity
//! by detecting phrase boundaries (energy valleys before downbeats).
//!
//! In produced music, phrases are 4, 8, 16, or 32 bars long.
//! The transition between phrases has a characteristic energy dip
//! ("svuoto") followed by a strong downbeat. If we find these dips
//! at regular intervals, we can confirm the metrical level.
//!
//! Algorithm:
//! 1. Compute low-frequency energy envelope (1 point per beat at candidate BPM)
//! 2. Find energy valleys (local minima below mean)
//! 3. Check if valleys are spaced at multiples of 4 bars (16 beats)
//! 4. Score how well the phrase structure fits at BPM vs BPM/2

/// Score how well a BPM candidate produces clean phrase boundaries.
///
/// Returns a score in [0, 1] where 1 = perfect phrase alignment.
/// Higher = more likely to be the correct metrical level.
pub fn phrase_score(samples: &[f32], sample_rate: u32, bpm: f64) -> f64 {
    if bpm <= 0.0 || samples.is_empty() {
        return 0.0;
    }

    let sr = sample_rate as f64;
    let beat_period = 60.0 / bpm; // seconds per beat
    let bar_period = beat_period * 4.0; // seconds per bar (4/4)
    let phrase_16 = bar_period * 16.0; // 16-bar phrase in seconds
    let duration = samples.len() as f64 / sr;

    // Need at least 2 phrases to detect structure
    if duration < phrase_16 * 2.0 {
        return 0.5; // neutral — can't determine
    }

    // Compute energy envelope: 1 point per bar
    let bar_samples = (bar_period * sr) as usize;
    if bar_samples == 0 {
        return 0.0;
    }
    let n_bars = samples.len() / bar_samples;
    if n_bars < 8 {
        return 0.5; // not enough bars
    }

    let mut bar_energy: Vec<f64> = Vec::with_capacity(n_bars);
    for i in 0..n_bars {
        let start = i * bar_samples;
        let end = (start + bar_samples).min(samples.len());
        let rms: f64 = samples[start..end]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum::<f64>()
            / (end - start) as f64;
        bar_energy.push(rms.sqrt());
    }

    // Normalize
    let max_e = bar_energy.iter().cloned().fold(0.0f64, f64::max);
    if max_e < 1e-10 {
        return 0.0;
    }
    for e in &mut bar_energy {
        *e /= max_e;
    }

    // Find energy valleys: bars below the median
    let mut sorted = bar_energy.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];

    let mut valleys: Vec<usize> = Vec::new();
    for i in 1..bar_energy.len().saturating_sub(1) {
        if bar_energy[i] < median * 0.7
            && bar_energy[i] <= bar_energy[i - 1]
            && bar_energy[i] <= bar_energy[i + 1]
        {
            valleys.push(i);
        }
    }

    if valleys.len() < 2 {
        return 0.5; // not enough valleys to judge — need at least 2 for distance
    }

    // Score: prefer BPMs where valley distances are EXACTLY 16 bars (standard phrase).
    // At the correct BPM: dips are 16 bars apart → distance = 16.
    // At double BPM: dips are 32 bars apart → distance = 32 (too long for 1 phrase).
    // Score = how many distances are close to 16 (ideal) vs 32 (suspicious doubling).
    let mut score_16 = 0.0f64;
    let mut total_pairs = 0;
    for i in 0..valleys.len() {
        for j in (i + 1)..valleys.len() {
            let dist = valleys[j] - valleys[i];
            if dist < 4 {
                continue;
            }
            total_pairs += 1;
            // Distance to nearest multiple of 16
            let nearest_16 = ((dist as f64 / 16.0).round() * 16.0) as usize;
            let err = (dist as isize - nearest_16 as isize).unsigned_abs();
            if err <= 2 {
                // Bonus for distances that are 16 (1 phrase) vs 32 (2 phrases)
                let phrase_count = nearest_16 / 16;
                // Prefer shorter phrases (16 > 32 > 48)
                score_16 += 1.0 / phrase_count as f64;
            }
        }
    }
    let aligned = score_16;

    if total_pairs == 0 {
        return 0.5;
    }

    aligned / total_pairs as f64
}

/// Resolve half/double ambiguity using phrase structure.
///
/// Compares phrase_score at `bpm` vs `bpm/2`. The one with better
/// phrase alignment is the correct metrical level.
///
/// Only applies when:
/// - Track is long enough (>= 90 seconds for meaningful phrase analysis)
/// - The half BPM is within the valid range
/// - There's a clear winner (score difference > 0.15)
pub fn resolve_halving(
    samples: &[f32],
    sample_rate: u32,
    bpm: f64,
    min_bpm: f64,
) -> f64 {
    let duration = samples.len() as f64 / sample_rate as f64;

    // Only for long-enough tracks and fast-enough BPMs
    // Range: only 140-200 BPM (the octave-doubling danger zone).
    // Duration: need >= 3 phrases of 16 bars at the candidate half-BPM.
    // At 140 BPM (half=70): 16 bars = 54.9s, 3 phrases = 165s minimum.
    // At 200 BPM (half=100): 16 bars = 38.4s, 3 phrases = 115s minimum.
    let half = bpm / 2.0;
    let phrase_secs = 60.0 / half * 4.0 * 16.0; // 16-bar phrase at half BPM
    let min_duration = phrase_secs * 3.5; // need ~3.5 phrases minimum (conservative)

    if !(140.0..=200.0).contains(&bpm) || half < min_bpm || duration < min_duration {
        return bpm;
    }

    let score_full = phrase_score(samples, sample_rate, bpm);
    let score_half = phrase_score(samples, sample_rate, bpm / 2.0);

    // Only halve if the half BPM has clearly better phrase structure
    // AND the full BPM doesn't have good phrase structure itself
    if score_half > score_full + 0.15 && score_full < 0.6 {
        bpm / 2.0
    } else {
        bpm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a signal with energy dips every `phrase_bars` bars.
    fn signal_with_phrases(bpm: f64, sr: u32, dur: f64, phrase_bars: usize) -> Vec<f32> {
        let s = sr as f64;
        let total = (dur * s) as usize;
        let beat = (60.0 / bpm * s) as usize;
        let bar = beat * 4;
        let phrase = bar * phrase_bars;
        let kick_n = (s * 0.05) as usize;

        let mut buf = vec![0.0f32; total];
        let mut pos = 0;
        let mut sample_pos = 0;
        while sample_pos < total {
            let in_phrase = sample_pos % phrase;
            let bar_in_phrase = in_phrase / bar;

            // Energy dip in the last bar of each phrase
            let is_dip = bar_in_phrase >= phrase_bars - 1;

            if !is_dip {
                // Normal kick
                for i in 0..kick_n.min(total - sample_pos) {
                    let t = i as f64 / s;
                    buf[sample_pos + i] += (2.0 * std::f64::consts::PI * 55.0 * t).sin() as f32
                        * (-(i as f64) / (kick_n as f64 * 0.2)).exp() as f32
                        * 0.8;
                }
            }
            // else: silence (the "svuoto")

            sample_pos += beat;
        }
        buf
    }

    #[test]
    fn test_phrase_score_16bar() {
        let samples = signal_with_phrases(128.0, 44100, 120.0, 16);
        let score = phrase_score(&samples, 44100, 128.0);
        assert!(
            score > 0.5,
            "16-bar phrase at 128 BPM should score well, got {}",
            score
        );
    }

    #[test]
    fn test_phrase_score_wrong_bpm() {
        let samples = signal_with_phrases(128.0, 44100, 120.0, 16);
        let score_correct = phrase_score(&samples, 44100, 128.0);
        let score_wrong = phrase_score(&samples, 44100, 137.0);
        assert!(
            score_correct >= score_wrong,
            "Correct BPM should have >= phrase score: {} vs {}",
            score_correct,
            score_wrong
        );
    }

    #[test]
    fn test_resolve_halving_slow_track() {
        // 80 BPM track with 16-bar phrases (phrase = 32s)
        // Detected as 160 BPM. Phrase analysis should prefer 80.
        let samples = signal_with_phrases(80.0, 44100, 180.0, 16);
        let resolved = resolve_halving(&samples, 44100, 160.0, 60.0);
        assert!(
            (resolved - 80.0).abs() < 1.0,
            "Should resolve 160→80, got {}",
            resolved
        );
    }

    #[test]
    fn test_resolve_halving_real_dnb() {
        // 174 BPM DnB with 16-bar phrases — should NOT halve
        let samples = signal_with_phrases(174.0, 44100, 120.0, 16);
        let resolved = resolve_halving(&samples, 44100, 174.0, 60.0);
        assert!(
            (resolved - 174.0).abs() < 1.0,
            "Should keep 174, got {}",
            resolved
        );
    }
}
