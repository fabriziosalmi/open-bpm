//! Tempo estimation: IOI histogram, comb filter, autocorrelation, and fusion.

use crate::onset::{self, Onset};
use crate::TempoEstimate;

// --- Constants ---

/// IOI histogram bin resolution in BPM.
const BIN_RESOLUTION: f64 = 0.25;
/// Maximum hops for multi-hop IOI.
const MAX_HOPS: usize = 8;
/// Gaussian smoothing sigma in bins.
const SMOOTH_SIGMA: f64 = 2.0;
/// Comb filter step size in BPM.
const COMB_STEP: f64 = 0.5;
/// Comb filter off-beat penalty factor.
const OFFBEAT_PENALTY: f64 = 0.3;
/// DJ-friendly BPM range bonus.
const DJ_RANGE_LO: f64 = 100.0;
const DJ_RANGE_HI: f64 = 185.0;
const DJ_RANGE_BONUS: f64 = 0.15;
/// Fusion agreement tolerance in BPM.
const FUSION_TOLERANCE: f64 = 4.0;
/// Harmonic voting weights.
const HARMONIC_SELF: f64 = 1.0;
const HARMONIC_DOUBLE: f64 = 0.7;
const HARMONIC_HALF: f64 = 0.7;

/// IOI histogram-based tempo estimation.
///
/// Computes multi-hop inter-onset intervals, bins them into a BPM histogram,
/// applies harmonic voting and Gaussian smoothing, then picks the peak.
pub fn ioi_histogram(onsets: &[Onset], min_bpm: f64, max_bpm: f64) -> Option<TempoEstimate> {
    if onsets.len() < 2 {
        return None;
    }

    let n_bins = ((max_bpm - min_bpm) / BIN_RESOLUTION) as usize + 1;
    let mut histogram = vec![0.0f64; n_bins];

    let bpm_to_bin = |bpm: f64| -> Option<usize> {
        if bpm < min_bpm || bpm > max_bpm {
            return None;
        }
        Some(((bpm - min_bpm) / BIN_RESOLUTION) as usize)
    };

    // Multi-hop IOI computation
    for i in 0..onsets.len() {
        for hop in 1..=MAX_HOPS.min(onsets.len() - i - 1) {
            let j = i + hop;
            if j >= onsets.len() {
                break;
            }

            let ioi = onsets[j].time - onsets[i].time;
            if ioi < 0.001 {
                continue;
            }

            // Convert to BPM (accounting for hop count)
            let bpm = 60.0 / (ioi / hop as f64);
            let weight = (onsets[i].strength + onsets[j].strength) * 0.5 / hop as f64;

            // Harmonic voting: vote for BPM, 2×BPM, and BPM/2
            for &(candidate, harmonic_weight) in &[
                (bpm, HARMONIC_SELF),
                (bpm * 2.0, HARMONIC_DOUBLE),
                (bpm / 2.0, HARMONIC_HALF),
            ] {
                if let Some(bin) = bpm_to_bin(candidate) {
                    if bin < n_bins {
                        histogram[bin] += weight * harmonic_weight;
                    }
                }
            }
        }
    }

    // Gaussian smoothing
    let smoothed = gaussian_smooth(&histogram, SMOOTH_SIGMA);

    // Find peak with parabolic interpolation
    find_peak_parabolic(&smoothed, min_bpm)
}

/// Comb filter resonator bank.
///
/// Tests a bank of comb filters at different BPM values against the onset
/// strength envelope. Uses float-precision beat periods to prevent phase drift.
pub fn comb_filter(
    onset_env: &[f64],
    sample_rate: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> Option<TempoEstimate> {
    if onset_env.is_empty() {
        return None;
    }

    let n = onset_env.len();
    let frame_rate = sample_rate / hop_size_for_sr(sample_rate as u32) as f64;
    let mut best_bpm = 0.0;
    let mut best_energy = 0.0f64;
    let mut total_energy = 0.0f64;

    let mut bpm = min_bpm;
    while bpm <= max_bpm {
        let period_frames = frame_rate * 60.0 / bpm; // FLOAT, not integer!

        // On-beat energy accumulation with linear interpolation
        let mut on_beat_energy = 0.0f64;
        let mut on_beat_count = 0usize;
        let mut pos = 0.0f64;
        while (pos as usize) < n.saturating_sub(1) {
            let idx = pos as usize;
            let frac = pos - idx as f64;
            let val = if idx + 1 < n {
                onset_env[idx] + frac * (onset_env[idx + 1] - onset_env[idx])
            } else {
                onset_env[idx]
            };
            on_beat_energy += val;
            on_beat_count += 1;
            pos += period_frames;
        }

        // Off-beat penalty (half-period suppression)
        let mut off_beat_energy = 0.0f64;
        let mut off_beat_count = 0usize;
        pos = period_frames / 2.0;
        while (pos as usize) < n.saturating_sub(1) {
            let idx = pos as usize;
            let frac = pos - idx as f64;
            let val = if idx + 1 < n {
                onset_env[idx] + frac * (onset_env[idx + 1] - onset_env[idx])
            } else {
                onset_env[idx]
            };
            off_beat_energy += val;
            off_beat_count += 1;
            pos += period_frames;
        }

        let normalized_on = if on_beat_count > 0 {
            on_beat_energy / on_beat_count as f64
        } else {
            0.0
        };
        let normalized_off = if off_beat_count > 0 {
            off_beat_energy / off_beat_count as f64
        } else {
            0.0
        };

        let energy = normalized_on - OFFBEAT_PENALTY * normalized_off;
        total_energy += energy.max(0.0);

        if energy > best_energy {
            best_energy = energy;
            best_bpm = bpm;
        }

        bpm += COMB_STEP;
    }

    if best_energy < 1e-10 {
        return None;
    }

    // Confidence: how much the best stands out from the mean
    let n_candidates = ((max_bpm - min_bpm) / COMB_STEP) as usize + 1;
    let mean_energy = total_energy / n_candidates as f64;
    let confidence = if mean_energy > 1e-10 {
        ((best_energy - mean_energy) / best_energy).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Some(TempoEstimate {
        bpm: best_bpm,
        confidence,
    })
}

/// Autocorrelation-based tempo estimation.
///
/// Computes the autocorrelation of the onset strength envelope,
/// finds peaks corresponding to beat periods, converts to BPM.
pub fn autocorrelation(
    onset_env: &[f64],
    sample_rate: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> Option<TempoEstimate> {
    if onset_env.len() < 4 {
        return None;
    }

    let frame_rate = sample_rate / hop_size_for_sr(sample_rate as u32) as f64;
    let min_lag = (frame_rate * 60.0 / max_bpm) as usize;
    let max_lag = (frame_rate * 60.0 / min_bpm) as usize;
    let max_lag = max_lag.min(onset_env.len() / 2);

    if min_lag >= max_lag || max_lag == 0 {
        return None;
    }

    // Compute mean
    let mean: f64 = onset_env.iter().sum::<f64>() / onset_env.len() as f64;

    // Compute autocorrelation for each lag
    let n = onset_env.len();
    let mut acf = vec![0.0f64; max_lag + 1];
    let mut acf0 = 0.0f64;

    // Normalization: autocorrelation at lag 0
    for i in 0..n {
        let v = onset_env[i] - mean;
        acf0 += v * v;
    }

    if acf0 < 1e-10 {
        return None;
    }

    for lag in min_lag..=max_lag {
        let mut sum = 0.0f64;
        for i in 0..n - lag {
            sum += (onset_env[i] - mean) * (onset_env[i + lag] - mean);
        }
        acf[lag] = sum / acf0;
    }

    // Apply perceptual weight (slight preference for common tempos)
    for lag in min_lag..=max_lag {
        let bpm = frame_rate * 60.0 / lag as f64;
        if bpm >= DJ_RANGE_LO && bpm <= DJ_RANGE_HI {
            acf[lag] *= 1.0 + DJ_RANGE_BONUS;
        }
    }

    // Find the global maximum of the weighted ACF.
    // NOTE: we do NOT require strict local-maximum here. For non-integer-period
    // signals (e.g. 180 BPM at 100 fps = 33.33 frames), the true peak falls
    // between integer lags. Requiring strict local max would miss the fundamental
    // and lock onto a harmonic at 2x the period (half the BPM).
    // Parabolic interpolation on the global max recovers sub-lag precision.
    let mut best_lag = min_lag;
    let mut best_val = acf[min_lag];
    for lag in min_lag + 1..=max_lag {
        if acf[lag] > best_val {
            best_val = acf[lag];
            best_lag = lag;
        }
    }

    // Parabolic interpolation for sub-sample precision
    let refined_lag = if best_lag > min_lag && best_lag < max_lag {
        let alpha = acf[best_lag - 1];
        let beta = acf[best_lag];
        let gamma = acf[best_lag + 1];
        let denom = 2.0 * (2.0 * beta - alpha - gamma);
        if denom.abs() > 1e-10 {
            best_lag as f64 + (alpha - gamma) / denom
        } else {
            best_lag as f64
        }
    } else {
        best_lag as f64
    };

    let bpm = frame_rate * 60.0 / refined_lag;

    if bpm < min_bpm || bpm > max_bpm {
        return None;
    }

    // Confidence: peak height relative to average ACF in range
    let avg_acf: f64 =
        acf[min_lag..=max_lag].iter().sum::<f64>() / (max_lag - min_lag + 1) as f64;
    let base_confidence = if avg_acf.abs() > 1e-10 {
        ((best_val - avg_acf) / best_val.abs()).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Octave ambiguity penalty: if ACF at 2*lag or lag/2 is strong relative
    // to the peak, the metrical level is ambiguous and confidence should drop.
    // This prevents AC from always dominating the fusion with ~98% confidence
    // when it can't actually distinguish 120 BPM from 60 BPM or 180 BPM.
    let mut ambiguity = 0.0f64;
    let double_lag = best_lag * 2;
    let half_lag = best_lag / 2;
    // Also check 3/2 and 2/3 lags (triplet ambiguity)
    let triplet_lag_up = best_lag * 4 / 3;
    let triplet_lag_down = best_lag * 3 / 4;

    for &check_lag in &[double_lag, half_lag, triplet_lag_up, triplet_lag_down] {
        if check_lag >= min_lag && check_lag <= max_lag && best_val > 1e-10 {
            let ratio = acf[check_lag] / best_val;
            ambiguity = ambiguity.max(ratio);
        }
    }

    // Scale: ambiguity=0 → no penalty, ambiguity=1 → halve confidence
    let confidence = (base_confidence * (1.0 - 0.5 * ambiguity)).clamp(0.0, 1.0);

    Some(TempoEstimate {
        bpm,
        confidence,
    })
}

/// Tempogram: windowed autocorrelation for tempo stability analysis.
///
/// Splits the onset envelope into overlapping windows and runs autocorrelation
/// on each. The BPM that appears most consistently across windows wins.
/// Confidence reflects temporal stability: high if all windows agree,
/// low if the tempo varies across the track.
pub fn tempogram(
    onset_env: &[f64],
    sample_rate: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> Option<TempoEstimate> {
    if onset_env.len() < 4 {
        return None;
    }

    let frame_rate = sample_rate / hop_size_for_sr(sample_rate as u32) as f64;
    let window_frames = (8.0 * frame_rate) as usize; // 8-second windows
    let hop_frames = window_frames / 2;              // 50% overlap

    if onset_env.len() < window_frames {
        // Track too short for windowed analysis — fall back to global AC
        return autocorrelation(onset_env, sample_rate, min_bpm, max_bpm);
    }

    let min_lag = (frame_rate * 60.0 / max_bpm) as usize;
    let max_lag = ((frame_rate * 60.0 / min_bpm) as usize).min(window_frames / 2);

    if min_lag >= max_lag {
        return None;
    }

    // Collect per-window BPM estimates
    let mut window_bpms: Vec<f64> = Vec::new();
    let mut pos = 0;

    while pos + window_frames <= onset_env.len() {
        let window = &onset_env[pos..pos + window_frames];

        // Compute mean
        let mean: f64 = window.iter().sum::<f64>() / window.len() as f64;

        // Autocorrelation at lag 0
        let mut acf0 = 0.0f64;
        for &v in window {
            let d = v - mean;
            acf0 += d * d;
        }

        if acf0 < 1e-10 {
            pos += hop_frames;
            continue;
        }

        // Find best lag
        let mut best_lag = min_lag;
        let mut best_val = f64::NEG_INFINITY;

        for lag in min_lag..=max_lag.min(window.len() / 2) {
            let mut sum = 0.0f64;
            for i in 0..window.len() - lag {
                sum += (window[i] - mean) * (window[i + lag] - mean);
            }
            let acf = sum / acf0;
            if acf > best_val {
                best_val = acf;
                best_lag = lag;
            }
        }

        // Parabolic interpolation
        let refined_lag = if best_lag > min_lag && best_lag < max_lag.min(window.len() / 2) {
            let lo_lag = best_lag - 1;
            let hi_lag = best_lag + 1;
            let mut sum_lo = 0.0f64;
            let mut sum_hi = 0.0f64;
            for i in 0..window.len() - hi_lag {
                let v = window[i] - mean;
                sum_lo += v * (window[i + lo_lag] - mean);
                sum_hi += v * (window[i + hi_lag] - mean);
            }
            let alpha = sum_lo / acf0;
            let beta = best_val;
            let gamma = sum_hi / acf0;
            let denom = 2.0 * (2.0 * beta - alpha - gamma);
            if denom.abs() > 1e-10 {
                best_lag as f64 + (alpha - gamma) / denom
            } else {
                best_lag as f64
            }
        } else {
            best_lag as f64
        };

        let bpm = frame_rate * 60.0 / refined_lag;
        if bpm >= min_bpm && bpm <= max_bpm {
            window_bpms.push(bpm);
        }

        pos += hop_frames;
    }

    if window_bpms.is_empty() {
        return None;
    }

    // Find the most common BPM (cluster by 4% tolerance)
    let mut best_bpm = window_bpms[0];
    let mut best_count = 0usize;

    for &candidate in &window_bpms {
        let count = window_bpms
            .iter()
            .filter(|&&b| (b - candidate).abs() / candidate < 0.04)
            .count();
        if count > best_count {
            best_count = count;
            best_bpm = candidate;
        }
    }

    // Refined BPM: median of the agreeing cluster
    let mut cluster: Vec<f64> = window_bpms
        .iter()
        .filter(|&&b| (b - best_bpm).abs() / best_bpm < 0.04)
        .copied()
        .collect();
    cluster.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_bpm = cluster[cluster.len() / 2];

    // Confidence = fraction of windows that agree with the majority
    let agreement = cluster.len() as f64 / window_bpms.len() as f64;
    let confidence = agreement.clamp(0.0, 1.0);

    Some(TempoEstimate {
        bpm: median_bpm,
        confidence,
    })
}

/// Spectral periodicity: FFT of the RMS energy envelope.
///
/// The simplest possible BPM detector: compute the energy envelope of
/// the full track (RMS in ~10ms windows), take its FFT, find the dominant
/// frequency in the BPM range. That frequency IS the tempo.
///
/// Completely independent from onset detection — sees the "breathing"
/// of the track (volume modulation) rather than the "hits" (transients).
/// This makes it orthogonal to all onset-based estimators.
pub fn spectral_energy_bpm(
    samples: &[f32],
    sample_rate: u32,
    min_bpm: f64,
    max_bpm: f64,
) -> Option<TempoEstimate> {
    use rustfft::{num_complex::Complex, FftPlanner};

    let sr = sample_rate as f64;
    let hop = hop_size_for_sr(sample_rate);
    let n_frames = samples.len() / hop;

    if n_frames < 64 {
        return None;
    }

    // Compute RMS energy envelope
    let mut envelope = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * hop;
        let end = (start + hop).min(samples.len());
        let rms: f64 = samples[start..end]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum::<f64>()
            / (end - start) as f64;
        envelope.push(rms.sqrt());
    }

    // Remove DC (mean) to focus on periodic variation
    let mean: f64 = envelope.iter().sum::<f64>() / envelope.len() as f64;
    for v in &mut envelope {
        *v -= mean;
    }

    // FFT of the energy envelope
    let fft_size = n_frames.next_power_of_two();
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut buffer: Vec<Complex<f64>> = envelope
        .iter()
        .map(|&v| Complex::new(v, 0.0))
        .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
        .take(fft_size)
        .collect();

    fft.process(&mut buffer);

    // Convert FFT bins to BPM and find the peak in [min_bpm, max_bpm]
    let frame_rate = sr / hop as f64;
    let min_bin = (min_bpm / 60.0 * fft_size as f64 / frame_rate).ceil() as usize;
    let max_bin = (max_bpm / 60.0 * fft_size as f64 / frame_rate).floor() as usize;
    let max_bin = max_bin.min(fft_size / 2);

    if min_bin >= max_bin {
        return None;
    }

    let mut best_bin = min_bin;
    let mut best_mag = 0.0f64;
    let mut total_mag = 0.0f64;

    for bin in min_bin..=max_bin {
        let mag = (buffer[bin].re * buffer[bin].re + buffer[bin].im * buffer[bin].im).sqrt();
        total_mag += mag;
        if mag > best_mag {
            best_mag = mag;
            best_bin = bin;
        }
    }

    if best_mag < 1e-10 {
        return None;
    }

    // Parabolic interpolation for sub-bin precision
    let refined_bin = if best_bin > min_bin && best_bin < max_bin {
        let alpha = (buffer[best_bin - 1].re.powi(2) + buffer[best_bin - 1].im.powi(2)).sqrt();
        let beta = best_mag;
        let gamma = (buffer[best_bin + 1].re.powi(2) + buffer[best_bin + 1].im.powi(2)).sqrt();
        let denom = 2.0 * (2.0 * beta - alpha - gamma);
        if denom.abs() > 1e-10 {
            best_bin as f64 + (alpha - gamma) / denom
        } else {
            best_bin as f64
        }
    } else {
        best_bin as f64
    };

    let bpm = refined_bin * frame_rate * 60.0 / fft_size as f64;

    let mean_mag = total_mag / (max_bin - min_bin + 1) as f64;
    let confidence = ((best_mag - mean_mag) / best_mag).clamp(0.0, 1.0);

    Some(TempoEstimate { bpm, confidence })
}

/// SBERN: Sub-Band Envelope Resonator Network (Hopf Oscillator Bank).
///
/// Instead of measuring onset intervals or combing for periodicity,
/// SBERN simulates 280 nonlinear oscillators (damped pendulums), each tuned
/// to a different BPM. The onset envelope "pushes" all oscillators
/// simultaneously. The oscillator that resonates with the largest amplitude
/// wins — it was pushed in-phase by the audio's periodic energy.
///
/// Key advantage: the Hopf oscillator has **inertia**. If a beat is missing
/// (breakdown, syncopation), the oscillator keeps swinging. If the next beat
/// arrives in-phase, it reinforces. This makes it robust to missing onsets,
/// polyrythms, and continuous modulation (sidechain, LFO wobble, drones).
///
/// Math: discrete Hopf oscillator
///   z[n+1] = z[n] + [(1 - |z[n]|²) * z[n] + i*w0*z[n] + eps*x[n]] * dt
///
/// Cost: 280 oscillators × N frames × ~10 ops = ~3M ops for 10 seconds.
pub fn hopf_oscillator_bank(
    onset_env: &[f64],
    sample_rate: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> Option<TempoEstimate> {
    if onset_env.len() < 10 {
        return None;
    }

    let frame_rate = sample_rate / hop_size_for_sr(sample_rate as u32) as f64;
    let dt = 1.0 / frame_rate;
    let epsilon = 0.05; // weak coupling — keeps oscillators in linear regime for max selectivity

    let step = COMB_STEP; // 0.5 BPM resolution
    let n_oscillators = ((max_bpm - min_bpm) / step) as usize + 1;

    // State: each oscillator is a complex number z = (re, im)
    let mut z_re = vec![0.001f64; n_oscillators]; // small seed, not zero
    let mut z_im = vec![0.0f64; n_oscillators];

    // Precompute rotation per frame: exact rotation by angle omega*dt
    // instead of Euler integration (prevents divergence at high omega)
    let cos_wdt: Vec<f64> = (0..n_oscillators)
        .map(|i| {
            let bpm = min_bpm + i as f64 * step;
            let omega = 2.0 * std::f64::consts::PI * bpm / 60.0;
            (omega * dt).cos()
        })
        .collect();
    let sin_wdt: Vec<f64> = (0..n_oscillators)
        .map(|i| {
            let bpm = min_bpm + i as f64 * step;
            let omega = 2.0 * std::f64::consts::PI * bpm / 60.0;
            (omega * dt).sin()
        })
        .collect();

    // Drive all oscillators and accumulate ABSORBED POWER.
    // Instead of reading final amplitude (which saturates at 1.0 for all),
    // we measure how much energy the signal transfers to each oscillator.
    // An in-phase oscillator absorbs more power than an out-of-phase one.
    let mut power = vec![0.0f64; n_oscillators];

    for &x in onset_env {
        for i in 0..n_oscillators {
            let re = z_re[i];
            let im = z_im[i];
            let mag_sq = re * re + im * im;

            // Nonlinear amplitude control
            let damped_re = re + (1.0 - mag_sq) * re * dt;
            let damped_im = im + (1.0 - mag_sq) * im * dt;

            // Exact rotation
            let rot_re = damped_re * cos_wdt[i] - damped_im * sin_wdt[i];
            let rot_im = damped_re * sin_wdt[i] + damped_im * cos_wdt[i];

            // Coupling
            let coupling = epsilon * x * dt;
            z_re[i] = rot_re + coupling;
            z_im[i] = rot_im;

            // Absorbed power: projection of coupling onto oscillator direction
            // P = coupling * cos(phase) = coupling * re / |z|
            let mag = (rot_re * rot_re + rot_im * rot_im).sqrt().max(1e-10);
            power[i] += (coupling * rot_re / mag).abs();
        }
    }

    // Find the oscillator that absorbed the most power
    let mut best_idx = 0;
    let mut best_amp = 0.0f64;
    let mut total_amp = 0.0f64;

    for i in 0..n_oscillators {
        let amp = power[i];
        total_amp += amp;
        if amp > best_amp {
            best_amp = amp;
            best_idx = i;
        }
    }

    if best_amp < 1e-10 {
        return None;
    }

    let bpm = min_bpm + best_idx as f64 * step;

    // Confidence: how much the winner stands out
    let mean_amp = total_amp / n_oscillators as f64;
    let confidence = ((best_amp - mean_amp) / best_amp).clamp(0.0, 1.0);

    Some(TempoEstimate { bpm, confidence })
}

/// Fuse three tempo estimates using **metrical-aware** clustering.
///
/// Two BPMs "agree" if they are within 4% of each other after normalizing
/// by common metrical ratios (1:1, 2:1, 1:2, 4:3, 3:4, 3:2, 2:3).
/// This lets IOI=126 and AC=168 cluster together (ratio 4:3).
///
/// When a cluster contains estimates at different metrical levels,
/// the output BPM is chosen by:
/// 1. Prefer the BPM in the strongest EDM tempo zone
/// 2. Ties broken by estimator priority: IOI > Comb > AC
///    (IOI measures onset intervals directly and is the most octave-reliable)
pub fn fuse_estimates(
    ioi: Option<TempoEstimate>,
    comb: Option<TempoEstimate>,
    ac: Option<TempoEstimate>,
    low_ac: Option<TempoEstimate>,
) -> TempoEstimate {
    // Tag each estimate with its source priority (lower = preferred for metrical choice)
    // IOI=0 (best octave), Comb=1, Low-band AC=1 (both octave-reliable), AC=3 (worst octave)
    let mut tagged: Vec<(TempoEstimate, u8)> = Vec::new();
    if let Some(e) = ioi {
        tagged.push((e, 0));
    }
    if let Some(e) = comb {
        tagged.push((e, 1));
    }
    if let Some(e) = low_ac {
        tagged.push((e, 1)); // spectral/low_ac: priority 1 (energy-based, octave-reliable)
    }
    if let Some(e) = ac {
        tagged.push((e, 3));
    }

    if tagged.is_empty() {
        return TempoEstimate { bpm: 0.0, confidence: 0.0 };
    }
    if tagged.len() == 1 {
        return tagged[0].0;
    }

    // Find the largest cluster where estimates agree metrically
    let mut best_cluster: Vec<usize> = Vec::new();
    let mut best_cluster_conf = 0.0f64;

    for i in 0..tagged.len() {
        let mut cluster = vec![i];
        for j in 0..tagged.len() {
            if i != j && bpm_agrees_metrical(tagged[i].0.bpm, tagged[j].0.bpm) {
                cluster.push(j);
            }
        }
        let conf: f64 = cluster.iter().map(|&idx| tagged[idx].0.confidence).sum();
        if cluster.len() > best_cluster.len()
            || (cluster.len() == best_cluster.len() && conf > best_cluster_conf)
        {
            best_cluster = cluster;
            best_cluster_conf = conf;
        }
    }

    if best_cluster.is_empty() {
        // No agreement at all: pick highest confidence
        return tagged
            .iter()
            .max_by(|a, b| a.0.confidence.partial_cmp(&b.0.confidence).unwrap())
            .unwrap()
            .0;
    }

    // Collect unique BPM candidates from the cluster (each estimate's raw BPM)
    let cluster_estimates: Vec<(TempoEstimate, u8)> =
        best_cluster.iter().map(|&idx| tagged[idx]).collect();

    // Choose the metrical level: score each candidate BPM
    let chosen_bpm = pick_metrical_level(&cluster_estimates);

    // Confidence: average of cluster + agreement bonus
    let avg_conf = cluster_estimates.iter().map(|(e, _)| e.confidence).sum::<f64>()
        / cluster_estimates.len() as f64;
    let agreement_bonus = match cluster_estimates.len() {
        3 => 0.15,
        2 => 0.05,
        _ => 0.0,
    };

    TempoEstimate {
        bpm: chosen_bpm,
        confidence: (avg_conf + agreement_bonus).clamp(0.0, 1.0),
    }
}

/// Common metrical ratios to test for agreement.
const METRICAL_RATIOS: &[f64] = &[1.0, 2.0, 0.5, 4.0 / 3.0, 3.0 / 4.0, 3.0 / 2.0, 2.0 / 3.0];

/// Check if two BPMs agree after normalizing by any common metrical ratio.
/// "Agree" means within 4% of each other.
fn bpm_agrees_metrical(a: f64, b: f64) -> bool {
    if a <= 0.0 || b <= 0.0 {
        return false;
    }
    let ratio = a / b;
    for &r in METRICAL_RATIOS {
        let normalized = ratio / r;
        if (normalized - 1.0).abs() < 0.04 {
            return true;
        }
    }
    false
}

/// Given a cluster of estimates (possibly at different metrical levels),
/// pick the best BPM value.
///
/// Strategy: collect all raw BPMs from the cluster, score them by
/// EDM tempo zone affinity, break ties by estimator priority (IOI first).
fn pick_metrical_level(cluster: &[(TempoEstimate, u8)]) -> f64 {
    if cluster.len() == 1 {
        return cluster[0].0.bpm;
    }

    // Check if all estimates agree directly (within 4 BPM) — simple average
    let all_direct = cluster.windows(2).all(|w| (w[0].0.bpm - w[1].0.bpm).abs() < FUSION_TOLERANCE);
    if all_direct {
        let total_w: f64 = cluster.iter().map(|(e, _)| e.confidence).sum();
        if total_w > 1e-10 {
            return cluster.iter().map(|(e, _)| e.bpm * e.confidence).sum::<f64>() / total_w;
        }
    }

    // Estimates are at different metrical levels.
    // Group by approximate BPM (within 4 BPM = same metrical level).
    // Pick the level with the most votes; break ties by EDM zone + confidence.
    let mut levels: Vec<(f64, usize, f64)> = Vec::new(); // (bpm, vote_count, total_score)

    for &(ref est, priority) in cluster {
        let zone = edm_tempo_zone_score(est.bpm);
        let priority_bonus = match priority {
            0 => 0.03,  // IOI
            1 => 0.02,  // Comb / low-band AC
            _ => 0.01,  // AC
        };
        let score = zone + est.confidence * 0.05 + priority_bonus;

        // Find existing level within 4 BPM
        let mut found = false;
        for level in levels.iter_mut() {
            if (level.0 - est.bpm).abs() < FUSION_TOLERANCE {
                level.1 += 1;
                level.2 += score;
                found = true;
                break;
            }
        }
        if !found {
            levels.push((est.bpm, 1, score));
        }
    }

    // Adaptive ranking: if any level has a strong EDM zone, zone dominates.
    // If no level has zone affinity, votes dominate (fall back to majority).
    //
    // This prevents the zone from overriding in low-BPM territory (< 100)
    // where zones don't exist, while still letting the zone correct
    // octave errors in the 120-180 range where zones are strong.
    let max_zone: f64 = levels.iter().map(|l| {
        // Extract zone component from total score (zone is the dominant part)
        edm_tempo_zone_score(l.0)
    }).fold(0.0f64, f64::max);

    let vote_weight = if max_zone > 0.15 {
        0.10 // zone available — let it dominate
    } else {
        0.40 // no zone — votes (majority) dominate
    };

    levels.sort_by(|a, b| {
        let rank_a = a.1 as f64 * vote_weight + a.2;
        let rank_b = b.1 as f64 * vote_weight + b.2;
        rank_b.partial_cmp(&rank_a).unwrap()
    });

    levels[0].0
}

/// Check if two BPM values agree (within tolerance, also checking octave relationships).
fn bpm_agrees(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() < tolerance
}

/// Post-fusion metrical sanity check.
///
/// The metrical-aware fusion handles octave/triplet resolution inside
/// `fuse_estimates()`. This function applies a simple stability prior
/// for octave (2x, /2) as a last safety net.
pub fn resolve_metrical(
    estimate: TempoEstimate,
    comb_est: Option<TempoEstimate>,
    _onset_env: &[f64],
    _sample_rate: f64,
    min_bpm: f64,
    max_bpm: f64,
) -> TempoEstimate {
    if estimate.bpm <= 0.0 {
        return estimate;
    }

    let bpm = estimate.bpm;
    let half = bpm / 2.0;
    let double = bpm * 2.0;

    let mut best_bpm = bpm;
    let mut best_score = -1.0f64;

    for &(candidate, is_original) in &[(half, false), (bpm, true), (double, false)] {
        if candidate < min_bpm || candidate > max_bpm {
            continue;
        }
        let mut score = 1.0;
        if is_original {
            score += 0.2;
        }
        if candidate >= DJ_RANGE_LO && candidate <= DJ_RANGE_HI {
            score += DJ_RANGE_BONUS * 0.3;
        }
        if score > best_score {
            best_score = score;
            best_bpm = candidate;
        }
    }

    // NOTE: comb filter override for sub-100 BPM was tested and reverted.
    // The comb says half-BPM in both correct cases (gt=85, comb=85) AND
    // incorrect cases (gt=140 halftime, comb=70). No reliable discriminant
    // was found to distinguish them — comb confidence is ~43% in both cases.

    TempoEstimate {
        bpm: best_bpm,
        confidence: estimate.confidence,
    }
}

/// Score how well a BPM falls in common EDM tempo zones.
///
/// Returns 0.0-1.0 based on proximity to known tempo attractors:
/// - House / deep house: 120-130
/// - Tech house / techno: 130-140
/// - Trance / hard techno: 140-150
/// - DnB / jungle: 170-178
/// - Half-time bass: 85-92
pub fn edm_tempo_zone_score(bpm: f64) -> f64 {
    let zones: &[(f64, f64, f64)] = &[
        // (center, half_width, peak_score)
        (105.0, 12.0, 0.5), // deep house / hip-hop / UK garage (93-117)
        (125.0, 8.0, 0.8),  // house
        (135.0, 8.0, 0.7),  // tech house
        (145.0, 8.0, 0.6),  // trance
        (174.0, 10.0, 0.7), // DnB (164-184)
        // NOTE: half-time zone (83-93) REMOVED — it systematically causes
        // correct 170+ BPM to be halved to 85. BPMs in this range are rare in
        // EDM and don't need zone protection.
        (155.0, 6.0, 0.4),  // hardstyle
        (190.0, 12.0, 0.4), // fast EDM / gabber (178-202)
    ];

    let mut best = 0.0f64;
    for &(center, half_width, peak) in zones {
        let dist = (bpm - center).abs();
        if dist < half_width {
            let score = peak * (1.0 - dist / half_width);
            best = best.max(score);
        }
    }
    best
}

/// Quick comb filter probe: returns the resonance score at a specific BPM.
fn comb_probe_score(onset_env: &[f64], sample_rate: f64, bpm: f64) -> f64 {
    let frame_rate = sample_rate / hop_size_for_sr(sample_rate as u32) as f64;
    let n = onset_env.len();
    let period_frames = frame_rate * 60.0 / bpm;

    if period_frames < 2.0 || n < 4 {
        return 0.0;
    }

    let mut on_energy = 0.0f64;
    let mut on_count = 0usize;
    let mut pos = 0.0f64;
    while (pos as usize) < n.saturating_sub(1) {
        let idx = pos as usize;
        let frac = pos - idx as f64;
        let val = if idx + 1 < n {
            onset_env[idx] + frac * (onset_env[idx + 1] - onset_env[idx])
        } else {
            onset_env[idx]
        };
        on_energy += val;
        on_count += 1;
        pos += period_frames;
    }

    let mut off_energy = 0.0f64;
    let mut off_count = 0usize;
    pos = period_frames / 2.0;
    while (pos as usize) < n.saturating_sub(1) {
        let idx = pos as usize;
        let frac = pos - idx as f64;
        let val = if idx + 1 < n {
            onset_env[idx] + frac * (onset_env[idx + 1] - onset_env[idx])
        } else {
            onset_env[idx]
        };
        off_energy += val;
        off_count += 1;
        pos += period_frames;
    }

    let norm_on = if on_count > 0 { on_energy / on_count as f64 } else { 0.0 };
    let norm_off = if off_count > 0 { off_energy / off_count as f64 } else { 0.0 };

    norm_on - OFFBEAT_PENALTY * norm_off
}

/// Legacy wrapper for tests that use the old API.
pub fn resolve_octave(
    estimate: TempoEstimate,
    _onsets: &[Onset],
    min_bpm: f64,
    max_bpm: f64,
) -> TempoEstimate {
    // Without onset envelope, just use stability prior
    if estimate.bpm <= 0.0 {
        return estimate;
    }

    let bpm = estimate.bpm;
    let half = bpm / 2.0;
    let double = bpm * 2.0;

    let mut best_bpm = bpm;
    let mut best_score = -1.0f64;

    for &(candidate, is_original) in &[(half, false), (bpm, true), (double, false)] {
        if candidate < min_bpm || candidate > max_bpm {
            continue;
        }
        let mut score = 1.0;
        if is_original {
            score += 0.2;
        }
        if candidate >= DJ_RANGE_LO && candidate <= DJ_RANGE_HI {
            score += DJ_RANGE_BONUS * 0.3;
        }
        if score > best_score {
            best_score = score;
            best_bpm = candidate;
        }
    }

    TempoEstimate {
        bpm: best_bpm,
        confidence: estimate.confidence,
    }
}

// --- Helper functions ---

/// Compute hop size for a given sample rate (target ~10ms).
fn hop_size_for_sr(sample_rate: u32) -> usize {
    ((sample_rate as f64 * 10.0 / 1000.0) as usize).max(1)
}

/// Smooth an envelope with a simple moving average for comb filter input.
pub fn smooth_envelope(data: &[f64], half_window: usize) -> Vec<f64> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut result = vec![0.0f64; data.len()];
    for i in 0..data.len() {
        let lo = i.saturating_sub(half_window);
        let hi = (i + half_window + 1).min(data.len());
        let sum: f64 = data[lo..hi].iter().sum();
        result[i] = sum / (hi - lo) as f64;
    }
    result
}

/// Gaussian smoothing of a 1D signal.
fn gaussian_smooth(data: &[f64], sigma: f64) -> Vec<f64> {
    let radius = (sigma * 3.0).ceil() as usize;
    let kernel_size = radius * 2 + 1;

    // Build kernel
    let kernel: Vec<f64> = (0..kernel_size)
        .map(|i| {
            let x = i as f64 - radius as f64;
            (-x * x / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let kernel_sum: f64 = kernel.iter().sum();
    let kernel: Vec<f64> = kernel.iter().map(|&k| k / kernel_sum).collect();

    // Convolve
    let mut result = vec![0.0f64; data.len()];
    for i in 0..data.len() {
        let mut sum = 0.0;
        for (j, &k) in kernel.iter().enumerate() {
            let idx = i as isize + j as isize - radius as isize;
            if idx >= 0 && (idx as usize) < data.len() {
                sum += data[idx as usize] * k;
            }
        }
        result[i] = sum;
    }
    result
}

/// Find the peak of a BPM histogram with parabolic interpolation.
fn find_peak_parabolic(histogram: &[f64], min_bpm: f64) -> Option<TempoEstimate> {
    if histogram.is_empty() {
        return None;
    }

    // Find global maximum
    let mut best_bin = 0;
    let mut best_val = histogram[0];
    for (i, &v) in histogram.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best_bin = i;
        }
    }

    if best_val < 1e-10 {
        return None;
    }

    // Parabolic interpolation
    let refined_bin = if best_bin > 0 && best_bin < histogram.len() - 1 {
        let alpha = histogram[best_bin - 1];
        let beta = histogram[best_bin];
        let gamma = histogram[best_bin + 1];
        let denom = 2.0 * (2.0 * beta - alpha - gamma);
        if denom.abs() > 1e-10 {
            best_bin as f64 + (alpha - gamma) / denom
        } else {
            best_bin as f64
        }
    } else {
        best_bin as f64
    };

    let bpm = min_bpm + refined_bin * BIN_RESOLUTION;

    // Confidence: peak relative to total
    let total: f64 = histogram.iter().sum();
    let confidence = if total > 0.0 {
        best_val / total
    } else {
        0.0
    };

    Some(TempoEstimate {
        bpm,
        confidence: confidence.clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::{Band, Onset};

    fn make_onsets(bpm: f64, duration: f64) -> Vec<Onset> {
        let period = 60.0 / bpm;
        let mut onsets = Vec::new();
        let mut t = 0.0;
        while t < duration {
            onsets.push(Onset {
                time: t,
                strength: 1.0,
                band: Band::Low,
            });
            t += period;
        }
        onsets
    }

    #[test]
    fn test_ioi_histogram_120bpm() {
        let onsets = make_onsets(120.0, 10.0);
        let result = ioi_histogram(&onsets, 60.0, 200.0);
        assert!(result.is_some());
        let est = result.unwrap();
        assert!(
            (est.bpm - 120.0).abs() < 2.0,
            "IOI: expected ~120, got {}",
            est.bpm
        );
    }

    #[test]
    fn test_ioi_histogram_140bpm() {
        let onsets = make_onsets(140.0, 10.0);
        let result = ioi_histogram(&onsets, 60.0, 200.0);
        assert!(result.is_some());
        let est = result.unwrap();
        assert!(
            (est.bpm - 140.0).abs() < 2.0,
            "IOI: expected ~140, got {}",
            est.bpm
        );
    }

    #[test]
    fn test_comb_filter_120bpm() {
        // Generate a simple onset envelope with peaks at 120 BPM
        let sr = 44100.0;
        let frame_rate = sr / 441.0; // 100 fps
        let n_frames = (frame_rate * 10.0) as usize;
        let beat_period_frames = frame_rate * 60.0 / 120.0;

        let mut env = vec![0.0f64; n_frames];
        let mut pos = 0.0;
        while (pos as usize) < n_frames {
            let idx = pos as usize;
            if idx < n_frames {
                env[idx] = 1.0;
                // Small gaussian around the beat
                for d in 1..5 {
                    if idx + d < n_frames {
                        env[idx + d] = (-((d as f64).powi(2)) / 4.0).exp();
                    }
                    if idx >= d {
                        env[idx - d] = (-((d as f64).powi(2)) / 4.0).exp();
                    }
                }
            }
            pos += beat_period_frames;
        }

        let result = comb_filter(&env, sr, 60.0, 200.0);
        assert!(result.is_some());
        let est = result.unwrap();
        assert!(
            (est.bpm - 120.0).abs() < 3.0,
            "Comb: expected ~120, got {}",
            est.bpm
        );
    }

    #[test]
    fn test_fuse_agreement() {
        let a = Some(TempoEstimate {
            bpm: 120.0,
            confidence: 0.8,
        });
        let b = Some(TempoEstimate {
            bpm: 121.0,
            confidence: 0.7,
        });
        let c = Some(TempoEstimate {
            bpm: 119.5,
            confidence: 0.6,
        });
        let fused = fuse_estimates(a, b, c, None);
        assert!(
            (fused.bpm - 120.0).abs() < 2.0,
            "Fused: expected ~120, got {}",
            fused.bpm
        );
        assert!(fused.confidence > 0.7, "Should have high confidence on agreement");
    }

    #[test]
    fn test_fuse_disagreement() {
        let a = Some(TempoEstimate {
            bpm: 120.0,
            confidence: 0.9,
        });
        let b = Some(TempoEstimate {
            bpm: 80.0,
            confidence: 0.3,
        });
        let c = Some(TempoEstimate {
            bpm: 160.0,
            confidence: 0.2,
        });
        let fused = fuse_estimates(a, b, c, None);
        // Should pick the highest confidence
        assert!(
            (fused.bpm - 120.0).abs() < 5.0,
            "Should pick ~120 (highest confidence), got {}",
            fused.bpm
        );
    }

    #[test]
    fn test_resolve_octave_preserves_original() {
        let est = TempoEstimate {
            bpm: 128.0,
            confidence: 0.8,
        };
        let onsets: Vec<Onset> = vec![Onset { time: 0.0, strength: 1.0, band: Band::Low }];
        let resolved = resolve_octave(est, &onsets, 60.0, 200.0);
        assert!(
            (resolved.bpm - 128.0).abs() < 1.0,
            "Should preserve original 128, got {}",
            resolved.bpm
        );
    }

    #[test]
    fn test_resolve_octave_stability_prior() {
        // Original BPM at 170 should stay at 170 (original bonus > DJ range for 85)
        let est = TempoEstimate {
            bpm: 170.0,
            confidence: 0.8,
        };
        let onsets: Vec<Onset> = vec![Onset { time: 0.0, strength: 1.0, band: Band::Low }];
        let resolved = resolve_octave(est, &onsets, 60.0, 200.0);
        assert!(
            resolved.bpm > 100.0,
            "Should keep 170 (stability prior), got {}",
            resolved.bpm
        );
    }

    #[test]
    fn test_gaussian_smooth() {
        let data = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let smoothed = gaussian_smooth(&data, 1.0);
        // Peak should be lower and spread out
        assert!(smoothed[3] < 1.0);
        assert!(smoothed[2] > 0.0);
        assert!(smoothed[4] > 0.0);
    }
}
