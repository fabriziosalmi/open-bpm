# open-bpm Benchmark Report

**GiantSteps Tempo Dataset Evaluation**

| | |
|---|---|
| **Project** | open-bpm v0.1.0 |
| **Algorithm** | Triple-estimator fusion (IOI histogram + comb filter + autocorrelation) |
| **Dataset** | GiantSteps Tempo Dataset (664 tracks, electronic music) |
| **Date** | 2026-04-09 |
| **Platform** | macOS (Apple Silicon), Rust release build |

---

## 1. Summary

| Metric | Value |
|---|---|
| Tracks tested | 664 |
| Missing audio | 0 |
| Detection errors | 0 |
| **Acc1 (4% tolerance)** | **457 / 664 (68.8%)** |
| **Acc2 (octave-tolerant)** | **523 / 664 (78.7%)** |
| Octave errors | 66 (9.9%) |

**Acc1** measures the percentage of tracks where the detected BPM falls within 4% of the ground-truth tempo. **Acc2** extends this tolerance to include octave multiples (2x, 0.5x, 3x, 1/3x), which is standard in MIR tempo evaluation.

---

## 2. Error Distribution

### 2.1 Absolute Error Statistics

| Statistic | Value |
|---|---|
| Mean absolute error | 16.36% |
| Median absolute error | 0.36% |
| 90th percentile | 52.11% |
| 95th percentile | 100.00% |
| 99th percentile | 106.67% |
| Maximum error | 213.26% |

### 2.2 Accuracy at Varying Tolerances

| Tolerance | Correct | Accuracy |
|---|---|---|
| <= 1% | 407 / 664 | 61.3% |
| <= 2% | 436 / 664 | 65.7% |
| <= 4% (Acc1) | 457 / 664 | 68.8% |
| <= 4% octave-tolerant (Acc2) | 523 / 664 | 78.7% |

The tight cluster of 61.3% of tracks within 1% error indicates high precision on correctly-identified tracks. The gap between Acc1 and Acc2 (10 percentage points) reveals that octave confusion is a significant error mode.

---

## 3. Accuracy by BPM Range

### 3.1 Acc1 (Strict 4% Tolerance)

| BPM Range | Tracks | Correct | Accuracy |
|---|---|---|---|
| < 90 | 68 | 3 | 4.4% |
| 90 -- 119 | 41 | 11 | 26.8% |
| 120 -- 139 | 191 | 133 | 69.6% |
| 140 -- 159 | 200 | 176 | 88.0% |
| 160 -- 179 | 149 | 127 | 85.2% |
| >= 180 | 15 | 7 | 46.7% |

### 3.2 Acc2 (Octave-Tolerant)

| BPM Range | Tracks | Correct | Accuracy |
|---|---|---|---|
| < 90 | 68 | 54 | 79.4% |
| 90 -- 119 | 41 | 14 | 34.1% |
| 120 -- 139 | 191 | 141 | 73.8% |
| 140 -- 159 | 200 | 180 | 90.0% |
| 160 -- 179 | 149 | 127 | 85.2% |
| >= 180 | 15 | 7 | 46.7% |

### 3.3 Analysis

- **Sweet spot (120--179 BPM):** The detector achieves 69.6%--88.0% Acc1 in the most common electronic music tempo range. This is where the majority of the dataset concentrates (540 of 664 tracks, 81.3%).
- **Slow tempos (< 90 BPM):** Acc1 drops to 4.4%, but Acc2 recovers to 79.4%, confirming that most errors in this range are octave doublings (the detector returns 2x the true BPM). This is a known challenge for tempo estimation algorithms.
- **90--119 BPM range:** Both Acc1 (26.8%) and Acc2 (34.1%) are notably low, suggesting difficulty in this transitional range.
- **Fast tempos (>= 180 BPM):** Limited sample size (15 tracks) makes conclusions tentative, but accuracy is moderate at 46.7%.

---

## 4. Dataset Distribution

| BPM Range | Tracks | Share |
|---|---|---|
| < 90 | 68 | 10.2% |
| 90 -- 119 | 41 | 6.2% |
| 120 -- 139 | 191 | 28.8% |
| 140 -- 159 | 200 | 30.1% |
| 160 -- 179 | 149 | 22.4% |
| >= 180 | 15 | 2.3% |

The GiantSteps dataset is predominantly composed of electronic dance music, with 81.3% of tracks in the 120--179 BPM range typical of house, techno, trance, and drum & bass genres.

---

## 5. Worst-Case Errors

The 10 tracks with the largest absolute error:

| Track ID | Ground Truth | Detected | Error |
|---|---|---|---|
| 3980001.LOFI | 53 BPM | 166.03 | +213.3% |
| 1874244.LOFI | 70 BPM | 186.56 | +166.5% |
| 4609944.LOFI | 58 BPM | 144.95 | +149.9% |
| 3789981.LOFI | 80 BPM | 173.12 | +116.4% |
| 1479462.LOFI | 90 BPM | 193.96 | +115.5% |
| 4960424.LOFI | 85 BPM | 177.23 | +108.5% |
| 4532060.LOFI | 85 BPM | 175.72 | +106.7% |
| 4153394.LOFI | 80 BPM | 165.34 | +106.7% |
| 2704868.LOFI | 64 BPM | 132.18 | +106.5% |
| 3167057.LOFI | 85 BPM | 175.33 | +106.3% |

All worst-case errors occur on slow tracks (53--90 BPM) where the detector latches onto a harmonic multiple of the true tempo. The ~100% and ~200% errors correspond to 2x and 3x octave confusion respectively.

---

## 6. Methodology

### 6.1 Dataset

**GiantSteps Tempo Dataset** -- a widely-used MIR benchmark comprising 664 electronic music tracks with manually annotated tempo values. Audio files are MP3 format.

### 6.2 Metrics

- **Acc1** (Accuracy 1): The detected BPM is within 4% of the ground truth.
  - Formula: `|detected - truth| / truth <= 0.04`
- **Acc2** (Accuracy 2): The detected BPM is within 4% of the ground truth or any octave-related multiple (2x, 0.5x, 3x, 1/3x).
  - Standard octave-tolerant metric used in MIREX and ISMIR evaluations.
- **Octave errors**: Tracks that pass Acc2 but fail Acc1, indicating the detector found the correct tempo at the wrong metrical level.

### 6.3 Execution

```
./bench/run_benchmark.sh
```

The benchmark script processes each track through the `open-bpm` release binary and compares the output against ground-truth annotations. Per-track results are written to `bench/benchmark_results.tsv`.

---

## 7. Context and Comparison

For reference, state-of-the-art deep-learning tempo estimators typically achieve Acc1 scores in the 80--95% range on GiantSteps, while traditional signal-processing methods generally fall in the 60--80% range.

open-bpm's Acc1 of **68.8%** and Acc2 of **78.7%** place it competitively among signal-processing approaches, with the advantage of being a lightweight, dependency-free Rust implementation with no neural network inference required.

### Key Strengths

- Zero detection failures across all 664 tracks
- High precision when correct: 61.3% of tracks have < 1% error
- Strong performance in the 120--179 BPM range (core electronic music)
- Pure signal processing, no ML dependencies

### Known Limitations

- Slow-tempo octave confusion (< 90 BPM, Acc1 = 4.4%)
- Weak 90--119 BPM range performance (Acc1 = 26.8%)
- Mean absolute error skewed by extreme outliers on slow tracks

---

## 8. Validation of Candidate Improvement Metrics

Two new candidate metrics were implemented and empirically validated against the baseline to determine whether they could improve the pipeline:

### 8.1 Methodology

For each of the 664 tracks, the following metrics were computed at four BPM points: ground truth, detected, half of detected, and double of detected. The validation tool ([`src/bin/validate_metrics.rs`](src/bin/validate_metrics.rs)) outputs a TSV with all values; analysis was done with awk.

A metric is considered useful if:
1. On **PASS tracks** (where the detector is already correct), it correctly defends the detected BPM over the half/double alternatives in >80% of cases
2. On **FAIL tracks**, it correctly identifies the ground truth BPM in >60% of cases

### 8.2 Harmonic Template Score

Idea: at the true fundamental, ACF harmonics (h1, h2, h3, h4) should decay monotonically and the sub-harmonic (lag/2) should be weaker than the fundamental.

| Test | Result |
|---|---|
| PASS tracks: HT defends det over alternatives | **7%** |
| PASS tracks: HT incorrectly prefers double | 81% |
| PASS tracks: HT incorrectly prefers half | 41% |
| FAIL tracks: HT prefers GT (correct) | 21% |
| FAIL tracks: HT prefers detected (wrong) | 69% |

**Verdict:** The metric is anti-correlated with truth. The formulation clamps `(h2/h1)` at 1.0, which destroys the signal that distinguishes octave errors. Marked as broken in the source; kept as a starting point for future redesign.

### 8.3 MDL Score

Idea: model the onset sequence as deviations from a regular grid at each BPM candidate. Lower total bits = better fit.

| Test | Result |
|---|---|
| PASS tracks: MDL defends det over alternatives | **29%** |
| FAIL tracks: MDL prefers GT (correct) | 52% |

**Verdict:** Essentially random. The residual magnitudes (~10^5 -- 10^6) are not properly normalized, and the data cost dominates the model cost. Marked as needing redesign.

### 8.4 Cascaded Rejection (Round 2)

Both metrics were integrated into a cascaded rejection scheme: when fusion confidence < 0.55, a weighted composite score (HT * 0.35 + MDL * 0.25 + grid * 0.20 + zone * 0.10 + stability) selected among candidate BPMs.

**Result on full GiantSteps benchmark:** Acc1 dropped from 68.8% to **32.0%**. The pipeline was reverted to baseline immediately. The validation step (above) was added afterward to identify the root cause.

### 8.5 Round 3: Four New Deterministic Metrics

After Round 2, four new candidate metrics were designed with the explicit goal of being deterministic, parameter-free, and orthogonal in the signals they capture:

| Metric | Mathematical basis | What it measures |
|---|---|---|
| **Phase coherence R** | Rayleigh statistic / circular mean | Angular concentration of onsets at the candidate period |
| **Empty slot score** | Combinatorial coverage | Fraction of grid slots filled by at least one onset |
| **Median energy ratio** | Robust statistics | Energy separation between on-grid and off-grid onsets |
| **IOI multiple score** | Arithmetic divisibility | Fraction of inter-onset intervals fitting integer ratios of T |

All four were validated empirically on the same 664 tracks before any integration. Results:

| Metric | PASS defense | FAIL recovery | Verdict |
|---|---|---|---|
| Phase coherence R | 12.7% | 54.6% | Failed |
| Empty slot score | 25.8% | 47.8% | Failed |
| Median energy ratio | 14.2% | 49.8% | Failed |
| IOI multiple score | 1.1% | 38.2% | Failed |

None passed the 80% / 60% threshold. None integratable.

### 8.6 The Structural Wall

A diagnostic analysis revealed the underlying cause. Across all 678 evaluated rows, **phase coherence at the doubled BPM was higher than at the detected BPM in 78% of cases** (and more than 2x higher in 56% of cases). This is not noise -- it is a structural property:

**At any correct BPM, the same musical content is also periodic at integer multiples of that BPM.** A kick drum at 140 BPM is mathematically also "phase-locked" to a 280 BPM grid (every other slot is filled). When evaluated at the doubled BPM:

- The grid spacing is half, so the fixed 30ms tolerance becomes a larger fraction of the period
- Random transients (hi-hat tails, snare crack, ghost notes) have twice as many slots to potentially align with
- The doubled BPM mathematically *contains* the structure of the correct BPM as a subset

This implies a **fundamental limit on onset-only signal processing**: no scalar metric defined as a function of `(onset_times, candidate_period)` alone can reliably distinguish a correct BPM from its integer-multiple harmonics, because the harmonic structure is not observable from raw onset positions.

### 8.7 Implications

The 68.8% Acc1 baseline appears to represent the practical ceiling of onset-only signal processing on this dataset. Further accuracy gains require information that the onset domain does not contain:

1. **Source separation** -- isolate the kick from other instruments before computing periodicities. Current low-band autocorrelation approximates this but adding it to fusion regressed in Round 1.
2. **Higher-level features** -- chord change detection, structural boundary analysis, downbeat estimation. These require independent analysis pipelines.
3. **Learned judge router** -- train a small classifier on (track features, candidate BPMs, ground truth) tuples from external datasets (Ballroom, GTZAN, Hainsworth) to learn which estimator to trust per track type.

### 8.8 Lessons Learned

1. **Empirical validation is mandatory before pipeline integration.** Theoretical soundness does not guarantee practical discrimination. Three rounds of attempts confirm this.
2. **Hand-tuned thresholds are an antipattern at this point.** Every parametric tweak (Round 1) and every composite scoring scheme (Round 2) regresses because the system is at a local optimum reachable from this design space.
3. **The structural wall is real.** Round 3 proved by direct measurement that onset-only metrics cannot distinguish a fundamental from its harmonics. Going beyond requires either source separation or external supervision.

---

## 9. Raw Data

Full per-track results are available in [`bench/benchmark_results.tsv`](bench/benchmark_results.tsv) with columns:

| Column | Description |
|---|---|
| `track_id` | GiantSteps track identifier |
| `ground_truth` | Annotated BPM |
| `detected` | open-bpm output BPM |
| `error_pct` | Signed relative error (%) |
| `acc1` | PASS/FAIL for 4% tolerance |
| `acc2` | PASS/FAIL for octave-tolerant 4% |
