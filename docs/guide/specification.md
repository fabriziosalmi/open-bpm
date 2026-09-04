# OpenBPM: Technical Specification

> Algorithmic reference for open-bpm v0.1.0

## 1. Signal Model

**Input**: mono PCM signal `x[n]`, sample rate `fs` (Hz), length `N` samples.

**Output**: `(bpm, confidence, grid_offset)` where:
- `bpm ∈ [min_bpm, max_bpm]`, precision 0.01 BPM
- `confidence ∈ [0, 1]`
- `grid_offset ∈ [0, 60/bpm)` seconds — time of first beat

---

## 2. Multi-Band Decomposition

Three parallel 2nd-order IIR biquad filters split the signal into frequency bands:

| Band | Type | Frequency | Musical content |
|------|------|-----------|----------------|
| Low | Lowpass | fc = 200 Hz, Q = 0.707 | Kick drum, bass |
| Mid | Bandpass | fc = 1000 Hz, Q = 1.0 | Snare, vocals, synths |
| High | Highpass | fc = 4000 Hz, Q = 0.707 | Hi-hat, cymbals, air |

### Biquad coefficients (Audio EQ Cookbook)

For lowpass at angular frequency `w0 = 2*pi*fc/fs`:

```
alpha = sin(w0) / (2*Q)

b0 = (1 - cos(w0)) / 2
b1 = (1 - cos(w0))
b2 = (1 - cos(w0)) / 2
a0 = 1 + alpha
a1 = -2 * cos(w0)
a2 = 1 - alpha
```

All coefficients normalized by `a0`. Anti-denormal: `x[n] += 1e-15` before processing.

Transfer function accuracy verified by lowpass energy reduction test (10 kHz component attenuated >30% through 250 Hz lowpass).

---

## 3. Onset Detection: SuperFlux

Based on Bock & Widmer (2013). Operates on each band independently.

### 3.1 STFT

- Window: Hann, length `W = 2048` samples
- Hop: `H = round(fs * 0.01)` samples (~10 ms, adapts to sample rate)
- FFT size: `W` (no zero-padding)
- Output: magnitude spectrogram `|X[f,t]|` with `K = W/2 + 1 = 1025` bins

Frame rate: `R = fs / H` (~100 fps at 44.1 kHz)

### 3.2 SuperFlux (frequency-axis max-filter)

For each frame `t > 0`:

```
SF[t] = sum over k of max(0, |X[k,t]| - max(|X[j,t-1]| for j in [k-1, k+1]))
```

The max-filter along the frequency axis of the **previous frame** suppresses spectral leakage from vibrato/tremolo — a bin at frequency `k` is only counted as a new onset if it exceeds the maximum of its frequency neighborhood in the previous frame.

Max-filter half-width: 1 bin (total width 3 bins = `max_filter_size`).

### 3.3 Adaptive Peak Picking

For each frame `t`, compute local statistics over a sliding window of ±10 frames:

```
mu[t] = mean(SF[t-10..t+10])
sigma[t] = stddev(SF[t-10..t+10])
threshold[t] = mu[t] + 1.5 * sigma[t]
```

An onset is detected at frame `t` if:
1. `SF[t] > threshold[t]` (above adaptive threshold)
2. `SF[t] > SF[t-1]` AND `SF[t] >= SF[t+1]` (local maximum)
3. Time since last onset > 60 ms (minimum IOI constraint)

Onset strength: `s = SF[t] - mu[t]`

### 3.4 Multi-Band Weighted Merge

Onsets from all three bands are collected and weighted:

| Band | Weight | Rationale |
|------|--------|-----------|
| Low (kick) | 2.0 | Primary beat marker in most music |
| Mid (snare) | 1.5 | Backbeat marker |
| High (hi-hat) | 0.5 | Subdivision, less relevant for tempo |

Onsets within 20 ms of each other are merged: strengths accumulate, band becomes `Merged`.

Output: sorted list of `(time, strength, band)` tuples.

---

## 4. Tempo Estimation

Three independent estimators run on different representations of the onset data.

### 4.1 IOI Histogram

**Input**: onset list `{(t_i, s_i)}`.

For each pair of onsets `(i, j)` where `j = i + hop`, `hop ∈ [1, 8]`:

```
IOI = t_j - t_i
BPM_candidate = 60 / (IOI / hop)
weight = min(s_i, s_j) / hop
```

Each candidate votes into a histogram with three harmonic components:

| Harmonic | Vote weight |
|----------|------------|
| BPM | 1.0 * weight |
| 2 * BPM | 0.7 * weight |
| BPM / 2 | 0.7 * weight |

Histogram parameters:
- Range: `[min_bpm, max_bpm]`
- Bin resolution: 0.25 BPM
- Post-processing: Gaussian smoothing (sigma = 2 bins, kernel radius = 6 bins)

Peak detection: global maximum with parabolic interpolation:

```
refined_bin = best_bin + (H[best-1] - H[best+1]) / (2 * (2*H[best] - H[best-1] - H[best+1]))
```

Confidence: `peak_value / sum(all bins)`.

### 4.2 Comb Filter Resonator Bank

**Input**: smoothed onset strength envelope (moving average, half-window = 5 frames).

For each candidate BPM (step 0.5, range `[min_bpm, max_bpm]`):

```
period = R * 60 / BPM    (FLOAT — critical for phase accuracy)
```

On-beat energy accumulation with **linear interpolation** (prevents quantization at high BPM):

```
E_on = (1/N_beats) * sum over k of lerp(env, k * period)
```

where `lerp(env, pos) = env[floor(pos)] + frac(pos) * (env[floor(pos)+1] - env[floor(pos)])`.

Off-beat penalty (suppresses half-time false positives):

```
E_off = (1/N_half) * sum over k of lerp(env, (k + 0.5) * period)
score = E_on - 0.3 * E_off
```

**Why float period matters**: at 170 BPM with R=100 fps, integer truncation loses 0.76 frames per beat. After 100 beats (~35 s), the comb drifts 76 frames and resonance dies. Float period maintains sub-frame phase accuracy indefinitely.

Confidence: `(best_score - mean_score) / best_score`.

### 4.3 Autocorrelation

**Input**: onset strength envelope (SuperFlux output, normalized to [0, 1]).

Normalized autocorrelation for lags corresponding to `[min_bpm, max_bpm]`:

```
lag_min = R * 60 / max_bpm
lag_max = min(R * 60 / min_bpm, N_frames / 2)

ACF[lag] = sum_t (env[t] - mu) * (env[t + lag] - mu) / sum_t (env[t] - mu)^2
```

DJ-range perceptual weighting: ACF values for lags corresponding to 100-185 BPM are scaled by `1.15`.

Peak detection: find local maxima (`ACF[lag] > ACF[lag-1]` AND `ACF[lag] >= ACF[lag+1]`), pick global best, refine with parabolic interpolation.

BPM from refined lag: `BPM = R * 60 / refined_lag`.

Confidence: `(peak - mean_ACF) / |peak|`, clamped to [0, 1].

---

## 5. Fusion

### 5.1 Agreement Check

Two estimates "agree" if `|BPM_a - BPM_b| < 3.0` BPM.

### 5.2 Cluster Selection

Find the largest cluster of mutually agreeing estimates. If tied, prefer the cluster containing the highest-confidence estimate.

### 5.3 Weighted Average

Within the agreeing cluster:

```
fused_BPM = sum(BPM_i * conf_i) / sum(conf_i)
fused_conf = mean(conf_i) + agreement_bonus
```

Agreement bonus: +0.15 if all 3 agree, +0.05 if 2 agree.

If no cluster has size > 1: pick the single highest-confidence estimate (no bonus).

---

## 6. Octave Resolution

Tests three candidates: `BPM/2`, `BPM`, `BPM*2` (each must be within `[min_bpm, max_bpm]`).

### Scoring

| Factor | Score delta | Condition |
|--------|-----------|-----------|
| Original estimate | +0.20 | Always (stability prior) |
| DJ range (100-185 BPM) | +0.045 | Candidate in range |
| Half-time detection | -0.15 | BPM > 140 AND kick/hat ratio < 0.4 AND not original |
| Double-time detection | -0.15 | BPM < 85 AND kick/hat ratio < 0.25 AND not original |

The **stability prior** (+0.20 for the original estimate) is deliberate: octave changes should only happen when there's clear evidence from band ratios. Without it, the DJ-range bonus alone can flip a correct 90 BPM to an incorrect 180 BPM.

Pick candidate with highest total score.

---

## 7. Fine Refinement

Grid alignment scoring over a ±2.5 BPM sweep at 0.1 BPM steps.

### Grid Alignment Score

For a candidate BPM and phase `phi`:

```
beat_period = 60 / BPM
tolerance = 0.12 * beat_period

For each onset (t_i, s_i) in first 80 onsets:
    delta = |t_i - phi| mod beat_period
    distance = min(delta, beat_period - delta) * beat_period / beat_period
    
    if distance < tolerance:
        x = distance / tolerance
        weight = (1 - x^2)^2          // Tukey window approximation of Gaussian
        score += s_i * weight
    
    total += s_i

normalized_score = score / total
```

For each candidate BPM, test up to 16 phase candidates (the first 16 onsets) and take the best score.

The refined BPM is the candidate with the highest grid alignment score.

---

## 8. Grid Offset (Phase-Locked Loop)

Find the time of the first beat.

Test 100 phase offsets uniformly distributed over one beat period, starting from the first onset:

```
For step in 0..100:
    phase = t_first + (step / 100) * beat_period
    energy = sum over onsets of: strength * (1 - x^2) where x = grid_distance / tolerance
    
best_offset = phase with max energy
final_offset = best_offset mod beat_period
```

Resolution: `beat_period / 100` (at 120 BPM: 5 ms; at 180 BPM: 3.3 ms).

---

## 9. Integer Snapping

If `|BPM - round(BPM)| < 0.15`:

```
score_original = grid_alignment_score(BPM)
score_rounded = grid_alignment_score(round(BPM))

if score_rounded >= 0.95 * score_original:
    BPM = round(BPM)
```

Rationale: most produced music has integer BPM. The 95% threshold prevents snapping when it would degrade grid fit.

---

## 10. Segmented Consensus

For tracks longer than `2 * segment_duration`:

1. Extract 3 segments of 15 seconds at positions 15%, 40%, 70% of track length
2. Run full pipeline on each segment independently
3. Cluster results: segments within 3 BPM of each other form a cluster
4. Take the largest cluster; weighted average by confidence
5. If all segments agree: +0.10 confidence bonus
6. If no segments agree: fall back to full-track analysis

This handles tracks with long intros, outros, or breakdowns that could skew a single-pass analysis.

---

## 11. Constants Reference

| Constant | Value | Module | Purpose |
|----------|-------|--------|---------|
| FFT_SIZE | 2048 | onset | STFT window length |
| HOP_TARGET | 10 ms | onset | STFT hop (adapts to sample rate) |
| MIN_IOI | 60 ms | onset | Minimum inter-onset interval |
| THRESHOLD_MULT | 1.5 | onset | Adaptive threshold = mean + 1.5*sigma |
| MERGE_WINDOW | 20 ms | onset | Multi-band onset deduplication |
| WEIGHT_LOW | 2.0 | onset | Kick onset weight |
| WEIGHT_MID | 1.5 | onset | Snare onset weight |
| WEIGHT_HIGH | 0.5 | onset | Hi-hat onset weight |
| LOW_CUTOFF | 200 Hz | onset | Low band filter |
| MID_CENTER | 1000 Hz | onset | Mid band center |
| HIGH_CUTOFF | 4000 Hz | onset | High band filter |
| BIN_RESOLUTION | 0.25 BPM | tempo | IOI histogram bin width |
| MAX_HOPS | 8 | tempo | Multi-hop IOI lookback |
| SMOOTH_SIGMA | 2.0 bins | tempo | Gaussian smoothing kernel |
| COMB_STEP | 0.5 BPM | tempo | Comb filter resolution |
| OFFBEAT_PENALTY | 0.3 | tempo | Half-period suppression |
| DJ_RANGE | 100-185 BPM | tempo | Perceptual preference zone |
| DJ_BONUS | 0.15 | tempo | Base bonus (attenuated in use) |
| FUSION_TOLERANCE | 4.0 BPM | tempo | Agreement threshold |
| HARMONIC_SELF | 1.0 | tempo | IOI self-vote weight |
| HARMONIC_DOUBLE | 0.7 | tempo | IOI octave-up vote weight |
| HARMONIC_HALF | 0.7 | tempo | IOI octave-down vote weight |
| GRID_TOLERANCE | 0.12 | beat | Fraction of beat period |
| REFINE_RANGE | ±2.5 BPM | beat | Fine sweep radius |
| REFINE_STEP | 0.1 BPM (coarse), 0.01 BPM (fine) | beat | Two-pass refinement |
| MAX_ONSETS_GRID | 80 | beat | Onsets used for grid scoring |
| MAX_PHASE | 16 | beat | Phase candidates per BPM |
| SNAP_THRESHOLD | 0.02 BPM | beat | Integer snap distance |
| SNAP_RATIO | 0.95 | beat | Min score ratio for snap |
| PLL_RESOLUTION | 100 | beat | Phase offset search granularity |
| SEGMENT_DURATION | 15 s | lib | Consensus segment length |
| NUM_SEGMENTS | 3 | lib | Consensus segment count |
| SEGMENT_POSITIONS | 15%, 40%, 70% | lib | Strategic analysis points |

---

## 12. Computational Complexity

| Stage | Complexity | Dominant cost |
|-------|-----------|---------------|
| Band filtering | O(N) | 3 biquad passes |
| STFT (per band) | O(N/H * W * log W) | FFT |
| SuperFlux | O(N/H * K * M) | K=1025 bins, M=3 max-filter |
| Peak picking | O(N/H * W_local) | W_local=20 sliding window |
| IOI histogram | O(P^2 * MAX_HOPS) | P = number of onsets |
| Comb filter | O(C * N/H) | C = (max-min)/step = 280 |
| Autocorrelation | O(L * N/H) | L = lag range |
| Grid refinement | O(50 * 16 * P) | 50 BPM steps * 16 phases |
| Total | O(N * log W) | STFT dominates |

Empirical: ~100 ms for 25 s track, ~250 ms for 8 min track (Apple M-series, release build).

---

## 13. Known Limitations

| Limitation | Cause | Mitigation |
|-----------|-------|------------|
| Assumes 4/4 time | Grid alignment and comb filter assume regular beats | Low confidence flag; future: meter detection |
| Single global BPM | Full-track averaging | Segmented analysis catches drift > 3 BPM |
| Octave errors on edge cases | 85 BPM vs 170, 75 vs 150 | Triple fusion + genre heuristics + stability prior |
| Low energy tracks | Ambient, minimal — few onsets detected | Confidence < 0.3 warns user |
| Swing / shuffle | Onsets don't align to straight grid | Grid tolerance (12%) absorbs mild swing |
| Intro anchoring | Grid may anchor to non-kick onset | PLL tests 100 phases; segmented analysis skips intro |

---

## 14. References

- Bock, S. & Widmer, G. (2013). "Maximum Filter Vibrato Suppression for Onset Detection." *Proc. DAFx*.
- Scheirer, E. (1998). "Tempo and Beat Analysis of Acoustic Musical Signals." *JASA*.
- Ellis, D. (2007). "Beat Tracking by Dynamic Programming." *JNMR*.
- Bock, S. et al. (2016). "madmom: A New Python Audio and Music Signal Processing Library." *Proc. ACM Multimedia*.
- Foscarin, F. et al. (2024). "Beat This! Accurate, Fast, and Easy-to-Use Beat Tracking." *Proc. ISMIR*.
- Audio EQ Cookbook, Robert Bristow-Johnson.
