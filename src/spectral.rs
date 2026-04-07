//! Spectral analysis: STFT, mel filterbank, spectral flux.

use rustfft::{num_complex::Complex, FftPlanner};
use std::f64::consts::PI;

/// Short-Time Fourier Transform result.
pub struct Stft {
    /// Magnitude spectrogram, shape: [n_frames][n_bins].
    pub magnitude: Vec<Vec<f64>>,
    /// Number of frequency bins (fft_size / 2 + 1).
    pub n_bins: usize,
    /// Hop size in samples.
    pub hop_size: usize,
    /// Frame rate (frames per second).
    pub frame_rate: f64,
}

/// Compute STFT magnitude spectrogram.
pub fn stft(samples: &[f32], sample_rate: u32, fft_size: usize, hop_size: usize) -> Stft {
    let n_bins = fft_size / 2 + 1;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    // Hann window
    let window: Vec<f64> = (0..fft_size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f64 / fft_size as f64).cos()))
        .collect();

    let n_frames = if samples.len() >= fft_size {
        (samples.len() - fft_size) / hop_size + 1
    } else {
        0
    };

    let mut magnitude = Vec::with_capacity(n_frames);
    let mut buffer = vec![Complex::new(0.0f64, 0.0); fft_size];

    for frame_idx in 0..n_frames {
        let start = frame_idx * hop_size;

        // Apply window and load into buffer
        for i in 0..fft_size {
            let sample = if start + i < samples.len() {
                samples[start + i] as f64
            } else {
                0.0
            };
            buffer[i] = Complex::new(sample * window[i], 0.0);
        }

        fft.process(&mut buffer);

        // Extract magnitude for positive frequencies
        let mag: Vec<f64> = buffer[..n_bins]
            .iter()
            .map(|c| (c.re * c.re + c.im * c.im).sqrt())
            .collect();
        magnitude.push(mag);
    }

    Stft {
        magnitude,
        n_bins,
        hop_size,
        frame_rate: sample_rate as f64 / hop_size as f64,
    }
}

/// SuperFlux onset detection function.
///
/// Computes spectral flux with maximum-filtering along the **frequency axis**
/// to suppress vibrato/tremolo artifacts. Based on Bock & Widmer (2013).
pub fn superflux(stft: &Stft, max_filter_size: usize) -> Vec<f64> {
    let n_frames = stft.magnitude.len();
    if n_frames < 2 {
        return vec![0.0; n_frames];
    }

    let half = max_filter_size / 2;

    // Half-wave rectified spectral flux where the previous frame's magnitude
    // is max-filtered along the frequency axis (suppresses vibrato).
    let mut flux = vec![0.0f64; n_frames];
    for frame in 1..n_frames {
        let mut sum = 0.0;
        for bin in 0..stft.n_bins {
            // Max-filter the previous frame along frequency axis
            let lo = bin.saturating_sub(half);
            let hi = (bin + half + 1).min(stft.n_bins);
            let mut prev_max = 0.0f64;
            for b in lo..hi {
                prev_max = prev_max.max(stft.magnitude[frame - 1][b]);
            }
            let diff = stft.magnitude[frame][bin] - prev_max;
            if diff > 0.0 {
                sum += diff;
            }
        }
        flux[frame] = sum;
    }

    flux
}

/// Simple spectral flux (half-wave rectified magnitude difference).
pub fn spectral_flux(stft: &Stft) -> Vec<f64> {
    let n_frames = stft.magnitude.len();
    if n_frames < 2 {
        return vec![0.0; n_frames];
    }

    let mut flux = vec![0.0f64; n_frames];
    for frame in 1..n_frames {
        let mut sum = 0.0;
        for bin in 0..stft.n_bins {
            let diff = stft.magnitude[frame][bin] - stft.magnitude[frame - 1][bin];
            if diff > 0.0 {
                sum += diff;
            }
        }
        flux[frame] = sum;
    }

    flux
}

/// Apply a simple 2nd-order IIR lowpass filter to mono samples.
/// Returns filtered samples. Cutoff in Hz.
pub fn lowpass_filter(samples: &[f32], sample_rate: u32, cutoff_hz: f64) -> Vec<f32> {
    biquad_filter(samples, sample_rate, cutoff_hz, BiquadType::Lowpass)
}

/// Apply a simple 2nd-order IIR highpass filter.
pub fn highpass_filter(samples: &[f32], sample_rate: u32, cutoff_hz: f64) -> Vec<f32> {
    biquad_filter(samples, sample_rate, cutoff_hz, BiquadType::Highpass)
}

/// Apply a bandpass filter.
pub fn bandpass_filter(samples: &[f32], sample_rate: u32, center_hz: f64, q: f64) -> Vec<f32> {
    biquad_filter_q(samples, sample_rate, center_hz, q, BiquadType::Bandpass)
}

#[derive(Clone, Copy)]
enum BiquadType {
    Lowpass,
    Highpass,
    Bandpass,
}

fn biquad_filter(samples: &[f32], sample_rate: u32, freq: f64, btype: BiquadType) -> Vec<f32> {
    biquad_filter_q(samples, sample_rate, freq, 0.707, btype)
}

fn biquad_filter_q(
    samples: &[f32],
    sample_rate: u32,
    freq: f64,
    q: f64,
    btype: BiquadType,
) -> Vec<f32> {
    let sr = sample_rate as f64;
    let w0 = 2.0 * PI * freq / sr;
    let alpha = w0.sin() / (2.0 * q);
    let cos_w0 = w0.cos();

    let (b0, b1, b2, a0, a1, a2) = match btype {
        BiquadType::Lowpass => {
            let b1 = 1.0 - cos_w0;
            let b0 = b1 / 2.0;
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w0;
            let a2 = 1.0 - alpha;
            (b0, b1, b2, a0, a1, a2)
        }
        BiquadType::Highpass => {
            let b1 = -(1.0 + cos_w0);
            let b0 = (1.0 + cos_w0) / 2.0;
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w0;
            let a2 = 1.0 - alpha;
            (b0, b1, b2, a0, a1, a2)
        }
        BiquadType::Bandpass => {
            let b0 = alpha;
            let b1 = 0.0;
            let b2 = -alpha;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w0;
            let a2 = 1.0 - alpha;
            (b0, b1, b2, a0, a1, a2)
        }
    };

    // Normalize
    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    let mut output = Vec::with_capacity(samples.len());
    let mut x1 = 0.0f64;
    let mut x2 = 0.0f64;
    let mut y1 = 0.0f64;
    let mut y2 = 0.0f64;

    for &s in samples {
        let x0 = s as f64 + 1e-15; // anti-denormal
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        output.push(y0 as f32);
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stft_basic() {
        let sr = 44100u32;
        let samples: Vec<f32> = (0..sr * 2)
            .map(|i| (2.0 * PI * 440.0 * i as f64 / sr as f64).sin() as f32)
            .collect();
        let result = stft(&samples, sr, 2048, 512);
        assert!(result.magnitude.len() > 0);
        assert_eq!(result.n_bins, 1025);
    }

    #[test]
    fn test_superflux_on_clicks() {
        let sr = 44100u32;
        let mut samples = vec![0.0f32; sr as usize * 2];
        // Place broadband clicks every 0.5 seconds (120 BPM)
        for i in 0..4 {
            let pos = (i as f64 * 0.5 * sr as f64) as usize;
            let click_len = 512;
            for j in 0..click_len {
                if pos + j < samples.len() {
                    let t = j as f64 / sr as f64;
                    let decay = (-(j as f64) / 80.0).exp() as f32;
                    samples[pos + j] = (2.0 * PI * 200.0 * t).sin() as f32 * decay * 0.9;
                }
            }
        }
        let s = stft(&samples, sr, 2048, 512);
        let flux = superflux(&s, 3);
        let max_flux = flux.iter().cloned().fold(0.0f64, f64::max);
        assert!(max_flux > 0.0);
    }

    #[test]
    fn test_lowpass_filter() {
        let sr = 44100u32;
        // Mix of 100 Hz and 10000 Hz
        let samples: Vec<f32> = (0..sr)
            .map(|i| {
                let t = i as f64 / sr as f64;
                ((2.0 * PI * 100.0 * t).sin() + (2.0 * PI * 10000.0 * t).sin()) as f32 * 0.5
            })
            .collect();
        let filtered = lowpass_filter(&samples, sr, 250.0);
        // After lowpass at 250 Hz, the 10 kHz component should be attenuated
        // Check energy in second half (after filter settles)
        let rms_original: f64 = samples[sr as usize / 2..]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum::<f64>()
            / (sr as f64 / 2.0);
        let rms_filtered: f64 = filtered[sr as usize / 2..]
            .iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum::<f64>()
            / (sr as f64 / 2.0);
        assert!(
            rms_filtered < rms_original * 0.7,
            "Lowpass should reduce energy"
        );
    }
}
