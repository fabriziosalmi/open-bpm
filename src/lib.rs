//! # open-bpm
//!
//! High-accuracy BPM detection using triple-estimator fusion.
//!
//! Combines three independent tempo estimation methods — IOI histogram,
//! comb filter resonator bank, and autocorrelation — then fuses their
//! results for robust, octave-error-resistant BPM detection.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use open_bpm::{detect, BpmResult};
//!
//! // samples: mono f32 PCM, sample_rate: e.g. 44100
//! let result = detect(&samples, sample_rate);
//! println!("BPM: {:.1} (confidence: {:.0}%)", result.bpm, result.confidence * 100.0);
//! ```

pub mod beat;
pub mod bouncer;
pub mod metalearner;
pub mod onset;
pub mod spectral;
pub mod tempo;

/// Result of BPM detection.
#[derive(Debug, Clone)]
pub struct BpmResult {
    /// Detected BPM (rounded to 0.01 precision).
    pub bpm: f64,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Grid offset: time in seconds of the first beat.
    pub grid_offset: f64,
    /// Per-estimator results for diagnostics.
    pub estimators: EstimatorResults,
}

/// Individual estimator results for transparency.
#[derive(Debug, Clone)]
pub struct EstimatorResults {
    pub ioi: Option<TempoEstimate>,
    pub comb: Option<TempoEstimate>,
    pub autocorrelation: Option<TempoEstimate>,
    pub low_band_ac: Option<TempoEstimate>,
    pub hopf: Option<TempoEstimate>,
    pub spectral: Option<TempoEstimate>,
    pub tempogram: Option<TempoEstimate>,
}

/// A single tempo estimate from one method.
#[derive(Debug, Clone, Copy)]
pub struct TempoEstimate {
    pub bpm: f64,
    pub confidence: f64,
}

/// Detection options.
#[derive(Debug, Clone)]
pub struct DetectOptions {
    /// Minimum BPM to consider (default: 60).
    pub min_bpm: f64,
    /// Maximum BPM to consider (default: 200).
    pub max_bpm: f64,
    /// Enable segmented analysis for variable-tempo tracks (default: true).
    pub segmented: bool,
    /// Number of analysis segments for consensus (default: 3).
    pub num_segments: usize,
    /// Segment duration in seconds (default: 15.0).
    pub segment_duration: f64,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self {
            min_bpm: 60.0,
            max_bpm: 200.0,
            segmented: true,
            num_segments: 3,
            segment_duration: 15.0,
        }
    }
}

/// Detect BPM with default options.
pub fn detect(samples: &[f32], sample_rate: u32) -> BpmResult {
    detect_with_options(samples, sample_rate, &DetectOptions::default())
}

/// Detect BPM with custom options.
pub fn detect_with_options(samples: &[f32], sample_rate: u32, opts: &DetectOptions) -> BpmResult {
    let sr = sample_rate as f64;
    let duration = samples.len() as f64 / sr;

    // If track is short or segmented is off, analyze the whole thing
    if !opts.segmented || duration < opts.segment_duration * 2.0 {
        return analyze_segment(samples, sample_rate, opts);
    }

    // Segmented analysis: pick strategic positions (skip intro/outro)
    let segment_len = (opts.segment_duration * sr) as usize;
    let positions: Vec<f64> = match opts.num_segments {
        1 => vec![0.3],
        2 => vec![0.25, 0.6],
        _ => vec![0.15, 0.40, 0.70],
    };

    let mut segment_results: Vec<BpmResult> = Vec::new();
    for &pos in &positions {
        let start = ((pos * samples.len() as f64) as usize).min(samples.len().saturating_sub(segment_len));
        let end = (start + segment_len).min(samples.len());
        if end - start < (sr * 3.0) as usize {
            continue; // skip segments shorter than 3 seconds
        }
        let result = analyze_segment(&samples[start..end], sample_rate, opts);
        segment_results.push(result);
    }

    if segment_results.is_empty() {
        return analyze_segment(samples, sample_rate, opts);
    }

    // Consensus: find the BPM most segments agree on
    consensus_merge(segment_results, samples, sample_rate, opts)
}

/// Analyze a single segment of audio.
fn analyze_segment(samples: &[f32], sample_rate: u32, opts: &DetectOptions) -> BpmResult {
    let sr = sample_rate as f64;

    // 0. Bouncer: extract acoustic passport (< 1ms)
    let onsets = onset::detect_onsets_multiband(samples, sample_rate);
    let onset_pairs: Vec<(f64, f64)> = onsets.iter().map(|o| (o.time, o.strength)).collect();
    let band_counts = onset::count_per_band(&onsets);
    let passport = bouncer::extract_passport(samples, sample_rate, &onset_pairs, band_counts);

    if onsets.len() < 4 {
        return BpmResult {
            bpm: 0.0,
            confidence: 0.0,
            grid_offset: 0.0,
            estimators: EstimatorResults {
                ioi: None,
                comb: None,
                autocorrelation: None,
                low_band_ac: None,
                hopf: None,
                spectral: None,
                tempogram: None,
            },
        };
    }

    // 2. Compute onset strength envelopes
    let onset_env = onset::onset_strength_envelope(samples, sample_rate);
    let low_env = onset::low_band_onset_envelope(samples, sample_rate);

    // 3. Multi-estimator tempo estimation
    let smoothed_env = tempo::smooth_envelope(&onset_env, 5);
    let ioi_est = tempo::ioi_histogram(&onsets, opts.min_bpm, opts.max_bpm);
    let comb_est = tempo::comb_filter(&smoothed_env, sr, opts.min_bpm, opts.max_bpm);
    let ac_est = tempo::autocorrelation(&onset_env, sr, opts.min_bpm, opts.max_bpm);
    // Low-band AC: autocorrelation on kick-only signal (immune to triplet hi-hats)
    let low_ac_est = tempo::autocorrelation(&low_env, sr, opts.min_bpm, opts.max_bpm);

    // Spectral energy BPM: FFT of RMS envelope (independent from onset detection)
    let spectral_est = tempo::spectral_energy_bpm(samples, sample_rate, opts.min_bpm, opts.max_bpm);

    // SBERN: Hopf oscillator bank
    let hopf_est = tempo::hopf_oscillator_bank(&onset_env, sr, opts.min_bpm, opts.max_bpm);

    // 4. Two-pass fusion:
    // Pass 1: onset-based estimators only
    let fused_onset = tempo::fuse_estimates(ioi_est, comb_est, ac_est, None);

    // Pass 2: if onset estimators disagree (low confidence), add spectral FFT
    let fused = if fused_onset.confidence < 0.60 {
        match spectral_est {
            Some(ref s) if s.confidence > 0.70 => {
                tempo::fuse_estimates(ioi_est, comb_est, ac_est, Some(*s))
            }
            _ => fused_onset,
        }
    } else {
        fused_onset
    };

    // 5. Metrical resolution (2x, /2, 4/3, 3/4)
    let resolved = tempo::resolve_metrical(
        fused,
        comb_est,
        &smoothed_env,
        sr,
        opts.min_bpm,
        opts.max_bpm,
    );

    // 5a. Hopf tiebreaker: when fusion confidence is low (estimators disagreed)
    // and the Hopf oscillator has a clear resonance in an EDM zone, trust it.
    // This catches the 16 tracks where all 3 core estimators are wrong but
    // the Hopf's nonlinear resonance locks onto the true period.
    let resolved = if resolved.confidence < 0.40 {
        if let Some(ref h) = hopf_est {
            let hopf_zone = tempo::edm_tempo_zone_score(h.bpm);
            if h.confidence > 0.10 && hopf_zone > 0.2 {
                TempoEstimate {
                    bpm: h.bpm,
                    confidence: resolved.confidence, // keep original confidence
                }
            } else {
                resolved
            }
        } else {
            resolved
        }
    } else {
        resolved
    };

    // 5b. Exclusion rules (Sherlock filter): eliminate impossible BPMs
    //
    // Rule N-3: if detected BPM > 160 AND D_high < 4/sec → NOT DnB/Jungle
    //   → force half-time (the fast BPM is a doubling artifact)
    // Rule N-4: if detected BPM < 80 AND D_low > 2/sec → NOT dubstep/trap
    //   → force double (the slow BPM is a halving artifact)
    let resolved = {
        let mut bpm = resolved.bpm;

        // N-3 and N-4 disabled for validation — need to verify on real audio first
        let _ = &passport; // suppress unused warning

        TempoEstimate {
            bpm,
            confidence: resolved.confidence,
        }
    };

    // 5b. Bar count tiebreaker — only for tracks >= 3 minutes where
    // track duration is musically meaningful (full songs produce cleaner
    // bar counts than short clips/previews)
    let track_duration = samples.len() as f64 / sr;
    let resolved = if track_duration >= 180.0 {
        let bar_resolved_bpm = beat::bar_count_resolve(
            resolved.bpm,
            track_duration,
            opts.min_bpm,
            opts.max_bpm,
        );
        TempoEstimate {
            bpm: bar_resolved_bpm,
            confidence: resolved.confidence,
        }
    } else {
        resolved
    };

    // 6. Grid alignment & fine refinement
    let (refined_bpm, grid_score) =
        beat::refine_bpm(&onsets, resolved.bpm, sr, opts.min_bpm, opts.max_bpm);

    // 7. Grid offset (PLL)
    let grid_offset = beat::find_grid_offset(&onsets, refined_bpm, sr);

    // 8. Snap to integer if close
    // Machine-timed tracks (DAW-produced) are ALWAYS integer BPM — wider snap
    let final_bpm = if passport.is_machine_timed {
        beat::snap_to_integer_wide(refined_bpm, &onsets, sr)
    } else {
        beat::snap_to_integer(refined_bpm, &onsets, sr)
    };

    // Confidence from fusion + grid alignment
    let confidence = (resolved.confidence * 0.6 + grid_score * 0.4).clamp(0.0, 1.0);

    BpmResult {
        bpm: (final_bpm * 100.0).round() / 100.0,
        confidence,
        grid_offset,
        estimators: EstimatorResults {
            ioi: ioi_est,
            comb: comb_est,
            autocorrelation: ac_est,
            low_band_ac: low_ac_est,
            hopf: hopf_est,
            spectral: spectral_est,
            tempogram: None,
        },
    }
}

/// Merge segment results by consensus.
fn consensus_merge(
    results: Vec<BpmResult>,
    samples: &[f32],
    sample_rate: u32,
    opts: &DetectOptions,
) -> BpmResult {
    let tolerance = 3.0; // BPM

    // Find clusters of agreeing estimates
    let mut best_cluster: Vec<&BpmResult> = Vec::new();

    for (i, r) in results.iter().enumerate() {
        let mut cluster: Vec<&BpmResult> = vec![r];
        for (j, r2) in results.iter().enumerate() {
            if i != j && (r.bpm - r2.bpm).abs() < tolerance {
                cluster.push(r2);
            }
        }
        if cluster.len() > best_cluster.len() {
            best_cluster = cluster;
        }
    }

    if best_cluster.is_empty() {
        // No consensus — fall back to full track analysis
        return analyze_segment(samples, sample_rate, opts);
    }

    // Weighted average of cluster
    let total_weight: f64 = best_cluster.iter().map(|r| r.confidence).sum();
    if total_weight < 1e-10 {
        return best_cluster[0].clone();
    }

    let avg_bpm: f64 = best_cluster
        .iter()
        .map(|r| r.bpm * r.confidence)
        .sum::<f64>()
        / total_weight;
    let avg_conf: f64 = best_cluster.iter().map(|r| r.confidence).sum::<f64>()
        / best_cluster.len() as f64;

    // Boost confidence if all segments agree
    let consensus_bonus = if best_cluster.len() == results.len() {
        0.1
    } else {
        0.0
    };

    // Use the grid offset from the highest-confidence segment
    let best_offset = best_cluster
        .iter()
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
        .map(|r| r.grid_offset)
        .unwrap_or(0.0);

    let best_estimators = best_cluster
        .iter()
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
        .map(|r| r.estimators.clone())
        .unwrap_or(EstimatorResults {
            ioi: None,
            comb: None,
            autocorrelation: None,
            low_band_ac: None,
            hopf: None,
            spectral: None,
            tempogram: None,
        });

    BpmResult {
        bpm: (avg_bpm * 100.0).round() / 100.0,
        confidence: (avg_conf + consensus_bonus).clamp(0.0, 1.0),
        grid_offset: best_offset,
        estimators: best_estimators,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_clicks(bpm: f64, sample_rate: u32, duration_secs: f64) -> Vec<f32> {
        let sr = sample_rate as f64;
        let total_samples = (duration_secs * sr) as usize;
        let beat_period_samples = (60.0 / bpm * sr) as usize;
        let click_len = (sr * 0.02) as usize; // 20ms click

        let mut samples = vec![0.0f32; total_samples];
        let mut pos = 0;
        while pos < total_samples {
            for i in 0..click_len.min(total_samples - pos) {
                let t = i as f64 / sr;
                // Exponentially decaying sine burst (kick-like, broadband)
                let decay = (-(i as f64) / (click_len as f64 * 0.3)).exp();
                samples[pos + i] = (2.0 * std::f64::consts::PI * 150.0 * t).sin() as f32
                    * decay as f32
                    * 0.9;
            }
            pos += beat_period_samples;
        }
        samples
    }

    #[test]
    fn test_120_bpm() {
        let samples = generate_clicks(120.0, 44100, 10.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - 120.0).abs() < 1.0,
            "Expected ~120 BPM, got {}",
            result.bpm
        );
    }

    #[test]
    fn test_140_bpm() {
        let samples = generate_clicks(140.0, 44100, 10.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - 140.0).abs() < 1.0,
            "Expected ~140 BPM, got {}",
            result.bpm
        );
    }

    #[test]
    fn test_90_bpm() {
        let samples = generate_clicks(90.0, 44100, 10.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - 90.0).abs() < 1.0,
            "Expected ~90 BPM, got {}",
            result.bpm
        );
    }

    #[test]
    fn test_174_bpm() {
        let samples = generate_clicks(174.0, 44100, 10.0);
        let result = detect(&samples, 44100);
        assert!(
            (result.bpm - 174.0).abs() < 2.0,
            "Expected ~174 BPM, got {}",
            result.bpm
        );
    }

    #[test]
    fn test_empty_input() {
        let result = detect(&[], 44100);
        assert_eq!(result.bpm, 0.0);
    }

    #[test]
    fn test_silence() {
        let samples = vec![0.0f32; 44100 * 10];
        let result = detect(&samples, 44100);
        assert!(result.confidence < 0.2, "Silence should have low confidence");
    }

    #[test]
    fn test_different_sample_rates() {
        for sr in [22050u32, 44100, 48000, 96000] {
            let samples = generate_clicks(128.0, sr, 10.0);
            let result = detect(&samples, sr);
            assert!(
                (result.bpm - 128.0).abs() < 2.0,
                "At {}Hz: expected ~128 BPM, got {}",
                sr,
                result.bpm
            );
        }
    }
}
