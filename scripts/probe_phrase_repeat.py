#!/usr/bin/env python3
"""Probe 1-bis: beat-aligned chromagram self-similarity.

For each track:
  1. Run beat tracking (librosa.beat.beat_track) → list of beat times
  2. Compute beat-synchronous chromagram (one 12-D vector per beat interval)
  3. For each shift k in {1, 2, 3, 4, 6, 8, 12, 16, 32}:
       similarity[k] = mean cosine similarity between chroma[i] and chroma[i+k]
  4. Output the best shift and its score, plus the full curve

Hypothesis being tested:
  - best_shift in {16, 32} with high score → EDM bar/phrase loop, label=0
  - best_shift in {3, 6} with high score → ternary meter (waltz), label=1
  - flat curve (no clear peak) → continuous variation (orchestral, jazz)

This avoids phrase delimiters entirely and works on tracks of any length
(as long as beat tracking succeeds).

Usage:
  probe_phrase_repeat.py <baseline.tsv> <audio_root> <layout> <dataset_name>
"""

import sys
import numpy as np
import librosa
from pathlib import Path


SR_TARGET = 22050
HOP_LENGTH = 512
SHIFTS = [1, 2, 3, 4, 6, 8, 12, 16, 32]


def find_audio(audio_root, track_id, layout):
    if layout == "flat":
        for ext in ("mp3", "wav"):
            p = Path(audio_root) / f"{track_id}.{ext}"
            if p.exists():
                return p
        return None
    else:
        root = Path(audio_root)
        for sub in root.iterdir():
            if sub.is_dir():
                for ext in ("wav", "mp3"):
                    p = sub / f"{track_id}.{ext}"
                    if p.exists():
                        return p
        return None


def beat_synchronous_chroma(y, sr):
    """Compute one 12-D chroma vector per beat interval.

    Returns:
        chroma_sync: shape (12, n_beats - 1) or None if beat tracking fails
        beat_times: array of beat times in seconds
        bpm_est: librosa's beat-tracker BPM estimate
    """
    try:
        tempo, beat_frames = librosa.beat.beat_track(
            y=y, sr=sr, hop_length=HOP_LENGTH
        )
    except Exception:
        return None, None, None

    if len(beat_frames) < 8:
        return None, None, None

    # Compute chromagram (CQT-based, more pitch-class accurate)
    try:
        chroma = librosa.feature.chroma_cqt(y=y, sr=sr, hop_length=HOP_LENGTH)
    except Exception:
        return None, None, None

    # Average chroma between consecutive beats → one vector per beat interval
    try:
        chroma_sync = librosa.util.sync(chroma, beat_frames, aggregate=np.mean)
    except Exception:
        return None, None, None

    # Drop the first column (before first beat) so each column = one beat interval
    if chroma_sync.shape[1] < 8:
        return None, None, None

    # Convert tempo (could be array in newer librosa)
    if hasattr(tempo, "__len__"):
        bpm_est = float(tempo[0])
    else:
        bpm_est = float(tempo)

    beat_times = librosa.frames_to_time(beat_frames, sr=sr, hop_length=HOP_LENGTH)
    return chroma_sync, beat_times, bpm_est


def shift_similarity(chroma_sync, k):
    """Mean cosine similarity between chroma[i] and chroma[i+k] over all i."""
    n = chroma_sync.shape[1]
    if k >= n:
        return 0.0
    a = chroma_sync[:, :-k]
    b = chroma_sync[:, k:]
    # Normalize columns
    a_norm = a / (np.linalg.norm(a, axis=0, keepdims=True) + 1e-12)
    b_norm = b / (np.linalg.norm(b, axis=0, keepdims=True) + 1e-12)
    sims = (a_norm * b_norm).sum(axis=0)
    return float(np.mean(sims))


def analyze_track(audio_path):
    try:
        y, sr = librosa.load(audio_path, sr=SR_TARGET, mono=True)
    except Exception:
        return None
    if len(y) < sr * 5:  # need at least 5 seconds
        return None

    chroma_sync, beat_times, bpm_est = beat_synchronous_chroma(y, sr)
    if chroma_sync is None:
        return None

    n_beats = chroma_sync.shape[1]

    # Compute similarity at each shift
    sim_curve = {}
    for k in SHIFTS:
        sim_curve[k] = shift_similarity(chroma_sync, k)

    # Best shift = argmax of the curve
    valid = {k: v for k, v in sim_curve.items() if k < n_beats}
    if not valid:
        return None
    best_k = max(valid, key=valid.get)
    best_score = valid[best_k]

    # Peak prominence: how much higher is the best shift than the median of others?
    other_scores = [v for k, v in valid.items() if k != best_k]
    median_others = float(np.median(other_scores)) if other_scores else 0.0
    prominence = best_score - median_others

    # Identify ternary structure: is shift 3 or 6 in the top 3?
    sorted_shifts = sorted(valid.items(), key=lambda kv: kv[1], reverse=True)
    top3 = [s for s, _ in sorted_shifts[:3]]
    is_ternary_top = int(any(s in (3, 6) for s in top3))

    # Identify EDM phrase structure: is shift 16 or 32 in the top 3?
    is_edm_phrase_top = int(any(s in (16, 32) for s in top3))

    return {
        "n_beats": n_beats,
        "bpm_librosa": bpm_est,
        "best_shift": best_k,
        "best_score": best_score,
        "prominence": prominence,
        "is_ternary_top": is_ternary_top,
        "is_edm_phrase_top": is_edm_phrase_top,
        "sim_curve": sim_curve,
    }


def main():
    if len(sys.argv) < 5:
        print("Usage: probe_phrase_repeat.py <baseline.tsv> <audio_root> <layout> <dataset_name>")
        sys.exit(1)

    baseline_path = sys.argv[1]
    audio_root = sys.argv[2]
    layout = sys.argv[3]
    dataset_name = sys.argv[4]

    # Header: include all 9 shift scores so we can analyze the full curve later
    shift_cols = "\t".join(f"sim_{k}" for k in SHIFTS)
    print(
        "track_id\tdataset\tgt_bpm\tdet_bpm\tlabel\t"
        "n_beats\tbpm_librosa\tbest_shift\tbest_score\tprominence\t"
        "is_ternary_top\tis_edm_phrase_top\t" + shift_cols
    )

    count = 0
    errors = 0
    with open(baseline_path) as f:
        next(f)
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 6:
                continue
            track_id = parts[0]
            try:
                gt_bpm = float(parts[1])
                det_bpm = float(parts[2])
            except ValueError:
                continue

            audio = find_audio(audio_root, track_id, layout)
            if audio is None:
                continue

            features = analyze_track(audio)
            if features is None:
                errors += 1
                continue

            candidates = [det_bpm, det_bpm / 2.0, det_bpm * 2.0, det_bpm * 3.0]
            label = -1
            for k, c in enumerate(candidates):
                if c > 0 and abs(c - gt_bpm) / gt_bpm <= 0.04:
                    label = k
                    break

            shift_vals = "\t".join(f"{features['sim_curve'][k]:.6f}" for k in SHIFTS)
            print(
                f"{track_id}\t{dataset_name}\t{gt_bpm}\t{det_bpm}\t{label}\t"
                f"{features['n_beats']}\t{features['bpm_librosa']:.2f}\t"
                f"{features['best_shift']}\t{features['best_score']:.6f}\t"
                f"{features['prominence']:.6f}\t"
                f"{features['is_ternary_top']}\t{features['is_edm_phrase_top']}\t"
                f"{shift_vals}",
                flush=True,
            )

            count += 1
            if count % 25 == 0:
                print(f"  ... {count} tracks processed", file=sys.stderr)

    print(f"Done: {count} tracks, {errors} errors", file=sys.stderr)


if __name__ == "__main__":
    main()
