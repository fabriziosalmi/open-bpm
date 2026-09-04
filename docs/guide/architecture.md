# open-bpm: Session Progress and Roadmap

**Last update:** 2026-04-11

## TL;DR

The judge router is **integrated and deployed** in the Rust pipeline.
End-to-end benchmark results on 2361 tracks across 3 datasets:

| Dataset | Before Router | After Router | Delta |
|---|---|---|---|
| GiantSteps (664) | 68.8% | 68.8% | +0 (zero regressions) |
| Ballroom (698) | 61.3% | **68.7%** | **+52 tracks (+7.4pp)** |
| GTZAN (999) | 59.6% | 59.4% | -1 (neutral) |

The Rust deployment uses a 32-feature model (without phrase probe features
which require librosa). The full 47-feature model achieved +143 in cross-
validation but the 32-feature model achieves +102 in CV and +52 end-to-end.

## Where we are in the larger arc

Six rounds of experimentation against the open-bpm Acc1 ceiling:

| Round | Idea | Result |
|---|---|---|
| 1 | 4 parametric tweaks (low_ac in fusion, comb probe, drop METRICAL_RATIOS, extend phrase halving) | -1.8pp; reverted |
| 2 | harmonic_template + MDL + cascaded rejection | -36.8pp catastrophe; reverted |
| 3 | 4 deterministic metrics (phase coherence R, empty slot, median energy, IOI multiple) validated empirically | All four below 80%/60% threshold; structural wall identified (onset-only metrics cannot distinguish a fundamental from its harmonics) |
| 4 | Logistic regression judge router on 39 features (GS+BB) | +100 in-dist CV but 0/-370 cross-dataset; classifier was a dataset detector |
| 5 | Round 4 + 11 phrase probe features (beat-aligned chroma similarity) | +110 in-dist (+10 vs Round 4); cross-dataset still 0/-403 |
| 6 (current) | Round 5 + GTZAN as third dataset (47 features, 1951 trainable, 3 dataset prior shift) | +143 in-dist; +46 zero-shot on BB; -445 zero-shot on GS |

The Round 6 result is the first authentic positive cross-dataset signal
in the entire session. It justifies proceeding to Rust integration.

## What is currently in the repo

### Datasets we use (all gitignored, must be re-downloaded)

- **GiantSteps Tempo Dataset:** 664 EDM tracks at `bench/giantsteps-audio/` and
  `bench/giantsteps-tempo-dataset/` (audio + annotations).
- **Ballroom:** 698 ballroom dance tracks at `~/mir_datasets/ballroom/B_1.0/`
  (downloaded via `mirdata`).
- **GTZAN:** 999 tracks across 10 genres at `~/mir_datasets/gtzan_genre/`
  (downloaded from HuggingFace mirror because the original Marsyas URL is
  permanently down). Tempo annotations from
  https://github.com/TempoBeatDownbeat/gtzan_tempo_beat as `annotations_tempo/`.

### Baseline benchmark numbers (open-bpm v0.1.0, no router)

| Dataset | Acc1 | Acc2 | Octave errors |
|---|---|---|---|
| GiantSteps (664) | 68.8% (457) | 78.7% (523) | 66 |
| Ballroom (698) | 61.3% (428) | 86.9% (607) | 179 |
| GTZAN (999) | 59.6% (595) | 83.4% (833) | 238 |
| **Total (2361)** | **63.0%** | **82.6%** | **483** |

### Generated artifacts

- `bench/baseline_results.tsv` -- canonical GiantSteps baseline (committed)
- `bench/ballroom_results.tsv` -- Ballroom benchmark output (committed)
- `bench/gtzan_results.tsv` -- GTZAN benchmark output (gitignored, regenerable)
- `bench/giantsteps_features.tsv` -- 39 judge features per track
- `bench/ballroom_features.tsv` -- same
- `bench/gtzan_features.tsv` -- same
- `bench/giantsteps_phrase.tsv` -- 11 phrase probe features per track
- `bench/ballroom_phrase.tsv` -- same
- `bench/gtzan_phrase.tsv` -- same
- `bench/combined_features.tsv` -- merged 2360-row table for training

### Code components

- `src/bin/extract_features.rs` -- emits per-track judge features TSV
  (39 columns: per-estimator BPM/conf, passport, Round 3 metrics)
- `src/bin/validate_metrics.rs` -- validates Round 3 metrics at GT/det/half/double
- `bench/run_ballroom_benchmark.sh` -- bash 3.2 compatible Ballroom benchmark
- `bench/run_gtzan_benchmark.sh` -- handles scientific notation annotations and
  GTZAN naming convention (`gtzan_<genre>_<num>.bpm` vs `<genre>.<num>.wav`)
- `scripts/probe_phrase_repeat.py` -- beat-aligned chromagram self-similarity
  (best_shift, prominence, sim curve at shifts {1,2,3,4,6,8,12,16,32})
- `scripts/extract_synthetic_features.py` -- 5 synthetic-ness features
  (sub_bass_ratio, spectral_flatness, attack_rise_time, mfcc_variance,
  contrast_variance) -- VALIDATED AS USELESS for label discrimination
  (it's a dataset detector, not a label predictor)
- `scripts/analyze_synthetic.py` -- prints distributions per dataset and label
- `scripts/analyze_phrase.py` -- same for phrase features
- `scripts/merge_features.py` -- joins judge + phrase TSVs across 3 datasets
- `scripts/train_judge_router.py` -- Round 4 logistic regression (legacy)
- `scripts/train_judge_router_v2.py` -- Round 5+6 with phrase features and
  3-dataset cross-validation splits

## Key empirical findings to NOT forget

1. **Onset-only metrics cannot distinguish a fundamental from its integer
   multiples.** Across 678 tracks, phase coherence at 2x-BPM is higher than
   at det-BPM in 78% of cases. This is mathematical, not noise. See
   BENCHMARK.md sec 8.6.

2. **Synthetic-ness features are dataset proxies, not label predictors.**
   sub_bass_ratio AUC for "GS vs BB" is 0.92, but Cohen's d for label
   discrimination within each dataset is essentially zero.

3. **Phrase probe (beat-aligned chroma self-similarity) DOES discriminate
   intra-dataset:** Cohen's d = -1.28 on Ballroom for label=0 vs label=1.
   This is the strongest signal we found. The mechanism: tracks where the
   detector doubled the BPM (Quickstep raddoppiati) have very high
   `prominence` because every "fake beat" is identical to the next, so the
   shift=1 cosine similarity dominates.

4. **The classifier needs to see at least one dataset SIMILAR to the target.**
   With GS+BB+GTZAN combined, in-distribution gain is +143 tracks.
   Zero-shot on Ballroom (train GS+GZ) gives +46. Zero-shot on GiantSteps
   (train BB+GZ) gives -445 because GiantSteps has a different label prior
   (90% label=0 vs 75% in BB+GZ).

## Completed steps (2026-04-10/11)

### Step 1 -- Export weights to Rust ✅
Trained on 1951 rows with 32 features (phrase features dropped since
librosa is not available in Rust). Exported via `scripts/export_weights.py`
to `src/judge_weights.rs`: 196 f64 constants (128 coef + 4 intercept +
64 scaler params). Training accuracy: 82.4%.

### Step 2 -- Judge router module ✅
`src/judge_router.rs`: pure-Rust softmax inference + threshold gating.
`RouterFeatures::build()` extracts 32 features from pipeline outputs.
Zero allocations at inference time.

### Step 3 -- Pipeline integration ✅
Router inserted in `detect_with_options()` after consensus merge + phrase
halving. Operates on the full track result (matching the Python training).
Recomputes onsets + passport on the full track for feature extraction.

### Step 4 -- End-to-end benchmark ✅
GS: 68.8% (unchanged), BB: 68.7% (+52), GZ: 59.4% (-1).

## What to do next (future work)

### Phrase probe port to Rust (recovers ~40 tracks)
- Cosine similarity at shifts {1,2,3,4,6,8,12,16,32}

This is a non-trivial port. **Alternative path** to consider: implement a
simpler chromagram-free probe in Rust that captures the same signal.
The Round 6 top features for Class 1 (halve) are:
  io_half (+1.80), transient_density (+1.64), io_det (+1.46), io_triple (+0.89),
  comb_conf (+0.66), n_onsets (-0.63), duration_s (+0.50), prominence (+0.48)

The current Rust model uses 32 features. A 47-feature model (including
phrase probe: beat-aligned chroma self-similarity, prominence, best_shift,
sim curve) scored +143 in cross-validation vs +102 for 32-feat. Porting
librosa's `beat_track` + `chroma_cqt` to Rust would recover ~40 tracks.
This is non-trivial but the top-contributing phrase feature (`prominence`)
accounts for most of the gap.

### Hainsworth as 4th dataset
222 non-EDM tracks (rock, pop, classical, folk). ~30 min total to download,
benchmark, extract features, and retrain. May improve generalization on
classical/jazz (currently 33% and 47% Acc1 on GTZAN).

### Lower threshold to recover GTZAN gains
Current threshold is 0.65. In cross-validation, t=0.55 gives +30 more
tracks on GTZAN but risks a few regressions on GiantSteps. Worth testing.

### Random forest as alternative model
A small RF (5-10 trees, depth 4) might capture non-linear feature
interactions. Harder to export to Rust (need a tree evaluator) but
could improve the 32-feature model without needing phrase features.

## Open architectural questions for future rounds

- **Is sklearn's LogisticRegression the right model class?** A small
  random forest (5-10 trees, depth 4) might capture non-linear interactions
  that linear models miss. But it's harder to export to Rust and the gain
  is uncertain. Try only if step 5 underperforms.

- **Should we incorporate librosa's beat tracker output as a separate
  estimator in the Rust fusion?** It's quite different from the existing
  estimators and could be a stronger source of `prominence`-like features.
  Costs adding a Rust beat tracker.

- **Source separation as a "structural wall breaker":** Round 3 proved
  that without source separation we cannot discriminate fundamental from
  harmonics on onset-only signals. The judge router avoids this by using
  many features at once, but a true breakthrough would need kick-only
  signal extraction. Way out of scope for now.

## Memory references

- `feedback_validate_metrics_first.md` -- always run validate_metrics.rs
  on new metrics before integration
- `project_open_bpm_status.md` -- ceiling reached, structural wall, multiple
  regression attempts (needs update for Round 6 success)
