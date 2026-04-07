//! Bouncer: ultra-fast acoustic passport extraction.
//!
//! Scans the raw signal in < 1ms to produce metadata that guides
//! the estimator ensemble. Not a BPM detector — a signal classifier.

/// Acoustic passport: metadata about the signal's character.
#[derive(Debug, Clone)]
pub struct AcousticPassport {
    /// Crest factor in dB. Low (< 6) = drone/wall of sound. High (> 20) = sparse transients.
    pub crest_factor_db: f32,
    /// Transient density: detected onsets per second.
    pub transient_density: f32,
    /// Tempo stability: 0.0 = chaotic/human, 1.0 = machine-perfect grid.
    pub tempo_stability: f32,
    /// True if signal appears to lack percussive transients.
    pub is_drumless: bool,
    /// True if timing suggests machine-sequenced (jitter < 2ms).
    pub is_machine_timed: bool,
    /// Low-band onset density (kicks/bass per second).
    pub d_low: f32,
    /// High-band onset density (hi-hats/cymbals per second).
    pub d_high: f32,
}

/// Extract acoustic passport from mono PCM samples.
/// Designed to run in < 1ms on any track length.
///
/// `band_counts`: (low_onsets, mid_onsets, high_onsets) from multi-band detection.
pub fn extract_passport(
    samples: &[f32],
    sample_rate: u32,
    onsets: &[(f64, f64)],
    band_counts: (usize, usize, usize),
) -> AcousticPassport {
    let sr = sample_rate as f64;
    let duration = samples.len() as f64 / sr;

    // 1. Crest factor: peak / RMS
    let (crest_factor_db, is_drumless) = compute_crest_factor(samples);

    // 2. Transient density: onsets per second
    let transient_density = if duration > 0.0 {
        onsets.len() as f32 / duration as f32
    } else {
        0.0
    };

    // 3. Tempo stability: jitter of inter-onset intervals
    let (tempo_stability, is_machine_timed) = compute_jitter(onsets);

    // 4. Per-band densities
    let (low_count, _mid_count, high_count) = band_counts;
    let d_low = if duration > 0.0 { low_count as f32 / duration as f32 } else { 0.0 };
    let d_high = if duration > 0.0 { high_count as f32 / duration as f32 } else { 0.0 };

    AcousticPassport {
        crest_factor_db,
        transient_density,
        tempo_stability,
        is_drumless,
        is_machine_timed,
        d_low,
        d_high,
    }
}

/// Crest factor = 20 * log10(peak / rms).
/// Runs on every 100th sample for speed.
fn compute_crest_factor(samples: &[f32]) -> (f32, bool) {
    if samples.is_empty() {
        return (0.0, true);
    }

    let step = 100.max(1); // subsample for speed
    let mut peak = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0usize;

    for i in (0..samples.len()).step_by(step) {
        let s = samples[i].abs() as f64;
        if s > peak {
            peak = s;
        }
        sum_sq += s * s;
        count += 1;
    }

    let rms = (sum_sq / count as f64).sqrt();
    let cf_db = if rms > 1e-10 {
        (20.0 * (peak / rms).log10()) as f32
    } else {
        0.0
    };

    // Drumless: crest factor < 6 dB means no sharp transients
    let is_drumless = cf_db < 6.0;

    (cf_db, is_drumless)
}

/// Inter-onset jitter: stddev of IOI in milliseconds.
/// Low jitter (< 2ms) = machine, high (> 5ms) = human.
fn compute_jitter(onsets: &[(f64, f64)]) -> (f32, bool) {
    if onsets.len() < 4 {
        return (0.5, false); // not enough data, assume medium stability
    }

    // Take up to 30 consecutive IOIs from the strongest onsets
    let n = onsets.len().min(31);
    let mut iois: Vec<f64> = Vec::new();
    for i in 1..n {
        let ioi = onsets[i].0 - onsets[i - 1].0;
        if ioi > 0.05 && ioi < 2.0 {
            // reasonable IOI range (30-2000 BPM)
            iois.push(ioi);
        }
    }

    if iois.len() < 3 {
        return (0.5, false);
    }

    // Find the most common IOI (mode) by clustering
    let mut best_ioi = iois[0];
    let mut best_count = 0;
    for &candidate in &iois {
        let count = iois.iter().filter(|&&x| (x - candidate).abs() < 0.02).count();
        if count > best_count {
            best_count = count;
            best_ioi = candidate;
        }
    }

    // Compute jitter: stddev of IOIs near the mode
    let near_mode: Vec<f64> = iois
        .iter()
        .filter(|&&x| (x - best_ioi).abs() < 0.05)
        .copied()
        .collect();

    if near_mode.len() < 3 {
        return (0.5, false);
    }

    let mean: f64 = near_mode.iter().sum::<f64>() / near_mode.len() as f64;
    let variance: f64 =
        near_mode.iter().map(|&x| (x - mean) * (x - mean)).sum::<f64>() / near_mode.len() as f64;
    let jitter_ms = variance.sqrt() * 1000.0; // convert to milliseconds

    // Stability: 1.0 at 0ms jitter, 0.0 at 10ms+
    let stability = (1.0 - jitter_ms / 10.0).clamp(0.0, 1.0) as f32;
    let is_machine = jitter_ms < 2.0;

    (stability, is_machine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crest_factor_sine() {
        // Pure sine: CF = 20*log10(sqrt(2)) ≈ 3.01 dB
        let sr = 44100u32;
        let samples: Vec<f32> = (0..sr)
            .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin() as f32)
            .collect();
        let (cf, drumless) = compute_crest_factor(&samples);
        assert!(cf > 2.5 && cf < 3.5, "Sine CF should be ~3dB, got {}", cf);
        assert!(drumless, "Sine should be drumless");
    }

    #[test]
    fn test_crest_factor_clicks() {
        // Sparse clicks: high CF
        let mut samples = vec![0.0f32; 44100];
        for i in (0..44100).step_by(4410) {
            samples[i] = 1.0;
        }
        let (cf, drumless) = compute_crest_factor(&samples);
        assert!(cf > 15.0, "Clicks should have high CF, got {}", cf);
        assert!(!drumless, "Clicks should not be drumless");
    }

    #[test]
    fn test_jitter_machine() {
        // Perfect machine timing: 120 BPM = 0.5s intervals
        let onsets: Vec<(f64, f64)> = (0..20)
            .map(|i| (i as f64 * 0.5, 1.0))
            .collect();
        let (stability, is_machine) = compute_jitter(&onsets);
        assert!(stability > 0.9, "Machine timing stability should be high, got {}", stability);
        assert!(is_machine, "Should detect as machine-timed");
    }

    #[test]
    fn test_jitter_human() {
        // Sloppy human timing: 120 BPM ± 10ms
        let mut state: u64 = 0xBEEF;
        let onsets: Vec<(f64, f64)> = (0..20)
            .map(|i| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let jitter = ((state >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.02;
                (i as f64 * 0.5 + jitter, 1.0)
            })
            .collect();
        let (stability, is_machine) = compute_jitter(&onsets);
        assert!(stability < 0.95, "Human timing should have lower stability, got {}", stability);
    }
}
