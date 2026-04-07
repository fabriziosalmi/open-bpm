# Progress

Benchmark results and iteration history for open-bpm.

Track drift between commits: if accuracy regresses, the delta shows immediately.

---

## Iteration Log

### v0.1.1 — 2026-04-07 — 4/4 Electronic Music Sprint

**Goal**: Perfect detection for 4/4 electronic music (House, Techno, DnB, Trance, etc.)

**Changes from mixi analysis**:
- IOI weight: `min(s_i, s_j)` → `(s_i + s_j) / 2` (average, less conservative — from mixi)
- Snap threshold: 0.15 → 0.02 BPM (tight, near-DAW precision)
- Two-pass refinement: coarse 0.1 BPM steps, then fine 0.01 BPM steps around winner
- Octave resolution: reverted from empirical grid score (biased against higher BPMs) to stability prior
- Fusion tie-breaking: by total confidence when cluster sizes equal (fixes 180 BPM → 90 bug)
- Fusion tolerance: 3.0 → 4.0 BPM (catches Comb+AC at 180 BPM)
- Autocorrelation: global max instead of strict local max (fixes non-integer-period signals)

**Draconian test suite**: 25/25 PASS (release: 2.25s)

| Test | v0.1.0 | v0.1.1 |
|------|--------|--------|
| Sweep 65-195 (27 pts) | PASS | PASS |
| Precision 30s (9 pts) | PASS | PASS |
| Fractional BPM | PASS | PASS |
| Sample rate invariance | PASS | PASS |
| Octave stability | PASS | PASS |
| Noise 20 dB | PASS | PASS |
| Noise 6 dB | PASS | PASS |
| Kick+hat pattern | PASS | PASS |
| Grid offset | PASS | PASS |
| Integer snap | PASS | PASS |
| All 25 tests | PASS | PASS |

**What didn't work (reverted)**:
- Empirical grid alignment for octave resolution: onset detectors miss beats at higher BPMs, so coverage systematically favors lower tempos → all high BPMs get halved

---

### v0.1.2 — 2026-04-07 — GiantSteps Benchmark + Triplet Correction

**Goal**: Benchmark on real EDM audio (GiantSteps Tempo, 664 tracks). Fix dominant error patterns.

**Changes**:
- Added `resolve_metrical()`: tests 3/4 and 4/3 candidates when the fused BPM is outside EDM tempo zones
- EDM tempo zone scoring: house (118-133), tech house (127-143), trance (137-153), DnB (168-180), deep house (102-118), half-time (83-93), hardstyle (149-161), fast EDM (178-202)
- Triplet correction: if `bpm * 3/4` or `bpm * 4/3` falls in a strong EDM zone and the original doesn't, override — but only if comb probe confirms resonance (>= 70% of original)
- Added `comb_probe_score()` for targeted BPM resonance testing

**GiantSteps Tempo Benchmark (664 EDM tracks)**:

```
Acc1 (4% tol):    365 / 664  (54.9%)
Acc2 (octave):    478 / 664  (71.9%)
Octave errors:    113 (17%)
```

Error breakdown:

| Error type | Count | % of failures |
|-----------|-------|---------------|
| 0.5x (half) | 75 | 25% |
| 2/3 (half + triplet) | 65 | 21% |
| 4/3 (triplet) | 49 | 16% |
| 2x (double) | 39 | 13% |
| Other | 47 | 15% |
| 3/4, 3/2, 4/5 | 24 | 8% |

**Draconian tests**: 25/25 PASS (51/51 total)

---

### v0.2.0 — 2026-04-07 — Metrical Resolution Overhaul

**Goal**: Fix the root cause of accuracy loss — the fusion step was letting autocorrelation (always ~98% confidence, often wrong octave) dominate over IOI histogram (correct octave 67-80% of the time).

**3 fixes applied:**

1. **AC confidence recalibration**: Octave ambiguity penalty — if ACF at 2x or 1/2 lag is strong relative to the peak, confidence drops. Prevents AC from always dominating fusion.

2. **Metrical-aware fusion** (biggest impact): `fuse_estimates()` now clusters estimates across metrical ratios (2x, /2, 4/3, 3/4, 3/2, 2/3). Two BPMs "agree" if within 4% after normalizing by any common ratio. When a cluster spans multiple metrical levels, majority vote picks the level; ties broken by EDM tempo zone score + estimator priority.

3. **Simplified resolve_metrical**: Removed fragile EDM zone heuristic (Phase 2 triplet correction). The metrical resolution now happens inside the fusion step. Post-fusion is just a stability prior for octave.

**GiantSteps Tempo Benchmark (664 EDM tracks):**

```
                v0.1.2      v0.2.0      Delta
Acc1 (4%):      54.9%       60.2%       +5.3
Acc2 (octave):  71.9%       75.1%       +3.2
Octave errors:  113         99          -14
Total failures: 299         264         -35
```

Error breakdown shift:

| Error | v0.1.2 | v0.2.0 | Change |
|-------|--------|--------|--------|
| 0.5x | 75 | 56 | -25% |
| 2/3x | 65 | 33 | **-49%** |
| 4/3x | 49 | 57 | +16% (tradeoff) |
| 2.0x | 39 | 45 | +15% (tradeoff) |
| 3/4x | 10 | 4 | -60% |
| other | 47 | 69 | +47% |

**Net: 35 fewer failures.** The 2/3x error (combined half+triplet) was nearly halved. The 4/3x and 2x increase is a tradeoff — the fusion is now more willing to change metrical level, which helps on average but creates new false positives.

**Draconian tests**: 25/25 PASS (51/51 total)

---

### v0.1.0 — 2026-04-07 — Initial release

**Algorithm**: Triple-estimator fusion (IOI histogram + comb filter + autocorrelation).

**Draconian test suite** (25 tests, release mode, 2.38s):

| Test | Status |
|------|--------|
| BPM sweep 65-195 (27 points, step 5) | PASS |
| Precision sweep 30s (9 points, tol 0.5 BPM) | PASS |
| Fractional BPM (120.5, 127.3, 133.33, 140.7, 174.25) | PASS |
| Sample rate invariance (22050, 44100, 48000, 88200, 96000) | PASS |
| Octave stability (70, 85, 90, 100, 128, 140, 150, 170, 175 BPM) | PASS |
| Noise robustness 20 dB (100, 120, 140, 170 BPM) | PASS |
| Noise robustness 6 dB (110, 128, 150 BPM) | PASS |
| Kick+hat pattern (120, 128, 140 BPM) | PASS |
| Grid offset accuracy | PASS |
| Confidence: silence < 0.2 | PASS |
| Confidence: noise < 0.4 | PASS |
| Confidence: clean > 0.3 | PASS |
| Confidence monotonic with duration | PASS |
| Integer snap exact | PASS |
| Estimator agreement on clean signal | PASS |
| Deterministic output | PASS |
| Performance 30s < 30s (debug) | PASS |
| DC offset immunity | PASS |
| Clipped signal no crash | PASS |
| NaN/Inf safety | PASS |
| Custom BPM range | PASS |
| No-segments mode | PASS |
| Empty input | PASS |
| Single sample | PASS |
| Sub-beat signal | PASS |

**Unit tests** (26 tests, 0.21s release):

All pass.

**Bugs fixed during iteration**:
- SuperFlux max-filter was along time axis (wrong) → fixed to frequency axis (Bock & Widmer 2013)
- Comb filter used sparse onset envelope → switched to smoothed onset envelope
- Comb filter confidence was `best/total` (meaningless with 280 candidates) → fixed to `(best-mean)/best`
- Fusion tie-breaking picked first estimate instead of highest confidence → fixed
- Fusion tolerance 3.0 BPM was too tight for Comb+AC at 180 BPM (3.36 apart) → raised to 4.0
- Autocorrelation required strict local max → removed (non-integer-period peaks missed)
- Octave resolution DJ-range bonus was too aggressive → added stability prior (+0.20 for original)

---

## Drift Detection

```
| Metric                         | v0.1.0 |
|--------------------------------|--------|
| Sweep pass rate (27 pts)       | 27/27  |
| Precision pass rate (9 pts)    | 9/9    |
| Octave errors                  | 0      |
| Mean confidence (clean 128)    | ~0.95  |
| Mean confidence (6 dB noise)   | ~0.30  |
| Draconian suite (release, s)   | 2.38   |
| Unit tests (release, s)        | 0.21   |
```

---

## Known Issues

- [ ] IOI histogram tends to vote for half-BPM at high tempos (>170 BPM) — harmonic voting at 0.7x weight accumulates. Mitigated by comb+AC fusion.
- [ ] Autocorrelation gives half-BPM at very high tempos (>185 BPM) — DJ-range bonus insufficient when ACF at 2T > ACF at T. Mitigated by comb+IOI fusion.
- [ ] Comb filter is the most reliable estimator across the full range but has lower confidence than autocorrelation.
- [ ] No real-audio benchmark dataset yet — need GTZAN/Ballroom/SMC ground truth comparison.
