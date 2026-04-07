//! Multi-band onset detection with SuperFlux and weighted merge.

use crate::spectral;

/// A detected onset event.
#[derive(Debug, Clone, Copy)]
pub struct Onset {
    /// Time in seconds.
    pub time: f64,
    /// Onset strength (arbitrary units, higher = stronger).
    pub strength: f64,
    /// Which band detected this onset.
    pub band: Band,
}

/// Frequency band classification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Band {
    Low,
    Mid,
    High,
    Merged,
}

// --- Constants ---

/// Minimum inter-onset interval in seconds (60ms).
const MIN_IOI_SECS: f64 = 0.06;
/// Adaptive threshold: stddev multiplier.
const THRESHOLD_STDDEV_MULT: f64 = 1.5;
/// Merge window for multi-band dedup (20ms).
const MERGE_WINDOW_SECS: f64 = 0.02;

/// Band weights for merged onsets.
const WEIGHT_LOW: f64 = 2.0; // kicks
const WEIGHT_MID: f64 = 1.5; // snares
const WEIGHT_HIGH: f64 = 0.5; // hi-hats

/// Filter crossover frequencies.
const LOW_CUTOFF: f64 = 200.0;
const MID_CENTER: f64 = 1000.0;
const MID_Q: f64 = 1.0;
const HIGH_CUTOFF: f64 = 4000.0;

/// STFT FFT size.
const FFT_SIZE: usize = 2048;

/// Detect onsets using multi-band SuperFlux with weighted merge.
pub fn detect_onsets_multiband(samples: &[f32], sample_rate: u32) -> Vec<Onset> {
    if samples.len() < FFT_SIZE {
        return Vec::new();
    }

    let hop = hop_size_for_sr(sample_rate);

    // Filter into 3 bands
    let low = spectral::lowpass_filter(samples, sample_rate, LOW_CUTOFF);
    let mid = spectral::bandpass_filter(samples, sample_rate, MID_CENTER, MID_Q);
    let high = spectral::highpass_filter(samples, sample_rate, HIGH_CUTOFF);

    // Detect onsets per band using SuperFlux
    let low_onsets = detect_band_onsets(&low, sample_rate, hop, Band::Low);
    let mid_onsets = detect_band_onsets(&mid, sample_rate, hop, Band::Mid);
    let high_onsets = detect_band_onsets(&high, sample_rate, hop, Band::High);

    // Weighted merge with deduplication
    merge_onsets(&low_onsets, &mid_onsets, &high_onsets)
}

/// Compute hop size scaled to sample rate (target ~10ms).
fn hop_size_for_sr(sample_rate: u32) -> usize {
    let target_ms = 10.0;
    ((sample_rate as f64 * target_ms / 1000.0) as usize).max(1)
}

/// Detect onsets in a single filtered band using SuperFlux.
fn detect_band_onsets(
    samples: &[f32],
    sample_rate: u32,
    hop: usize,
    band: Band,
) -> Vec<Onset> {
    let stft_result = spectral::stft(samples, sample_rate, FFT_SIZE, hop);
    let flux = spectral::superflux(&stft_result, 3);

    peak_pick_adaptive(&flux, stft_result.frame_rate, band)
}

/// Adaptive peak picking with variance-based threshold.
fn peak_pick_adaptive(flux: &[f64], frame_rate: f64, band: Band) -> Vec<Onset> {
    if flux.is_empty() {
        return Vec::new();
    }

    let min_ioi_frames = (MIN_IOI_SECS * frame_rate) as usize;
    let half_window = 10usize; // frames for local statistics

    // Compute local mean and stddev using sliding window
    let n = flux.len();
    let mut onsets = Vec::new();
    let mut last_onset_frame: Option<usize> = None;

    for i in 1..n.saturating_sub(1) {
        // Local window
        let lo = i.saturating_sub(half_window);
        let hi = (i + half_window + 1).min(n);
        let window_len = (hi - lo) as f64;

        let mean: f64 = flux[lo..hi].iter().sum::<f64>() / window_len;
        let variance: f64 =
            flux[lo..hi].iter().map(|&v| (v - mean) * (v - mean)).sum::<f64>() / window_len;
        let stddev = variance.sqrt();

        let threshold = mean + THRESHOLD_STDDEV_MULT * stddev;

        // Peak: above threshold AND local maximum
        if flux[i] > threshold && flux[i] > flux[i - 1] && flux[i] >= flux[i + 1] {
            // Enforce minimum IOI
            if let Some(last) = last_onset_frame {
                if i - last < min_ioi_frames {
                    continue;
                }
            }

            let strength = flux[i] - mean;
            if strength > 0.0 {
                onsets.push(Onset {
                    time: i as f64 / frame_rate,
                    strength,
                    band,
                });
                last_onset_frame = Some(i);
            }
        }
    }

    onsets
}

/// Merge onsets from three bands with weighted deduplication.
fn merge_onsets(low: &[Onset], mid: &[Onset], high: &[Onset]) -> Vec<Onset> {
    // Collect all onsets with band weights applied
    let mut all: Vec<Onset> = Vec::with_capacity(low.len() + mid.len() + high.len());

    for o in low {
        all.push(Onset {
            strength: o.strength * WEIGHT_LOW,
            band: o.band,
            ..*o
        });
    }
    for o in mid {
        all.push(Onset {
            strength: o.strength * WEIGHT_MID,
            band: o.band,
            ..*o
        });
    }
    for o in high {
        all.push(Onset {
            strength: o.strength * WEIGHT_HIGH,
            band: o.band,
            ..*o
        });
    }

    // Sort by time
    all.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());

    // Merge onsets within MERGE_WINDOW_SECS
    let mut merged: Vec<Onset> = Vec::new();
    for onset in &all {
        if let Some(last) = merged.last_mut() {
            if onset.time - last.time < MERGE_WINDOW_SECS {
                // Merge: keep highest strength, accumulate
                last.strength += onset.strength;
                last.band = Band::Merged;
                // Keep the time of the stronger onset
                continue;
            }
        }
        merged.push(*onset);
    }

    merged
}

/// Compute onset strength envelope (for autocorrelation-based tempo estimation).
///
/// Returns a 1D signal at ~100 Hz frame rate representing onset likelihood.
pub fn onset_strength_envelope(samples: &[f32], sample_rate: u32) -> Vec<f64> {
    let hop = hop_size_for_sr(sample_rate);
    let stft_result = spectral::stft(samples, sample_rate, FFT_SIZE, hop);
    let flux = spectral::superflux(&stft_result, 3);

    // Normalize
    let max = flux.iter().cloned().fold(0.0f64, f64::max);
    if max < 1e-10 {
        return flux;
    }
    flux.iter().map(|&v| v / max).collect()
}

/// Compute onset strength envelope of the **low band only** (< 200 Hz).
///
/// This captures kick drum periodicity without hi-hat/snare interference.
/// Immune to triplet hi-hat patterns that cause 4/3 errors in the full-spectrum
/// onset envelope.
pub fn low_band_onset_envelope(samples: &[f32], sample_rate: u32) -> Vec<f64> {
    let low = spectral::lowpass_filter(samples, sample_rate, LOW_CUTOFF);
    let hop = hop_size_for_sr(sample_rate);
    let stft_result = spectral::stft(&low, sample_rate, FFT_SIZE, hop);
    let flux = spectral::superflux(&stft_result, 3);

    // Normalize
    let max = flux.iter().cloned().fold(0.0f64, f64::max);
    if max < 1e-10 {
        return flux;
    }
    flux.iter().map(|&v| v / max).collect()
}

/// Compute RMS energy envelope (for comb filter resonator bank).
///
/// Returns a continuous energy signal at ~100 Hz frame rate.
/// Unlike the sparse onset envelope, this gives the comb filter
/// a smooth signal to resonate against.
pub fn energy_envelope(samples: &[f32], sample_rate: u32) -> Vec<f64> {
    let hop = hop_size_for_sr(sample_rate);
    let n_frames = if samples.len() > hop {
        samples.len() / hop
    } else {
        return Vec::new();
    };

    let mut env = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * hop;
        let end = (start + hop).min(samples.len());
        let rms: f64 = samples[start..end]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum::<f64>()
            / (end - start) as f64;
        env.push(rms.sqrt());
    }

    // Normalize
    let max = env.iter().cloned().fold(0.0f64, f64::max);
    if max > 1e-10 {
        for v in &mut env {
            *v /= max;
        }
    }

    env
}

/// Count onsets per band (for genre heuristics in octave resolution).
pub fn count_per_band(onsets: &[Onset]) -> (usize, usize, usize) {
    let mut low = 0;
    let mut mid = 0;
    let mut high = 0;
    for o in onsets {
        match o.band {
            Band::Low => low += 1,
            Band::Mid => mid += 1,
            Band::High => high += 1,
            Band::Merged => {
                // Count as all bands
                low += 1;
                mid += 1;
                high += 1;
            }
        }
    }
    (low, mid, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_onsets_synthetic_kicks() {
        let sr = 44100u32;
        let duration = 5.0;
        let bpm = 120.0;
        let beat_period = 60.0 / bpm;
        let total_samples = (duration * sr as f64) as usize;

        // Generate kick-like pulses (low frequency bursts)
        let mut samples = vec![0.0f32; total_samples];
        let mut t = 0.0;
        while t < duration {
            let pos = (t * sr as f64) as usize;
            let click_len = (sr as f64 * 0.01) as usize;
            for i in 0..click_len {
                if pos + i < total_samples {
                    let phase = i as f64 / sr as f64;
                    samples[pos + i] =
                        (2.0 * std::f64::consts::PI * 80.0 * phase).sin() as f32
                            * (1.0 - i as f32 / click_len as f32)
                            * 0.9;
                }
            }
            t += beat_period;
        }

        let onsets = detect_onsets_multiband(&samples, sr);
        // Should detect approximately 10 onsets (120 BPM * 5 sec = 10 beats)
        assert!(
            onsets.len() >= 6 && onsets.len() <= 16,
            "Expected ~10 onsets for 120 BPM over 5s, got {}",
            onsets.len()
        );
    }

    #[test]
    fn test_onset_strength_envelope() {
        let sr = 44100u32;
        let samples: Vec<f32> = (0..sr * 2)
            .map(|i| {
                let t = i as f64 / sr as f64;
                // Click at every 0.5s
                let beat_pos = (t * 2.0).floor();
                let in_click = t * 2.0 - beat_pos < 0.01;
                if in_click { 0.8 } else { 0.0 }
            })
            .collect();

        let env = onset_strength_envelope(&samples, sr);
        assert!(!env.is_empty());
        let max = env.iter().cloned().fold(0.0f64, f64::max);
        assert!(max > 0.0);
    }
}
