#!/usr/bin/env python3
"""Extract 'synthetic-ness' features from audio.

For each track, computes 5 deterministic signal-processing features that
should distinguish synthesized music (DnB, techno, 808 kicks) from acoustic
music (waltz, classical, live drums):

  1. sub_bass_ratio    -- energy 20-80 Hz / total energy
  2. spectral_flatness -- median Wiener entropy across frames
  3. attack_rise_time  -- median onset attack rise time (samples)
  4. mfcc_variance     -- mean variance of MFCC coefficients across time
  5. inharmonicity     -- median deviation of spectral peaks from harmonic series

Higher sub_bass_ratio + lower mfcc_variance + lower spectral_flatness = "synthetic".
Output: TSV with track_id, dataset, 5 features, label.

Usage:
  extract_synthetic_features.py <baseline.tsv> <audio_root> <layout> <dataset_name>
    layout: flat | subdir
"""

import sys
import os
import numpy as np
import librosa
from pathlib import Path


def find_audio(audio_root, track_id, layout):
    if layout == "flat":
        for ext in ("mp3", "wav"):
            p = Path(audio_root) / f"{track_id}.{ext}"
            if p.exists():
                return p
        return None
    else:
        # subdir: walk one level
        root = Path(audio_root)
        for sub in root.iterdir():
            if sub.is_dir():
                for ext in ("wav", "mp3"):
                    p = sub / f"{track_id}.{ext}"
                    if p.exists():
                        return p
        return None


def compute_features(audio_path, sr_target=22050):
    """Return dict of 5 synthetic-ness features."""
    try:
        y, sr = librosa.load(audio_path, sr=sr_target, mono=True)
    except Exception as e:
        return None

    if len(y) < sr_target:  # less than 1 second
        return None

    n_fft = 2048
    hop = 512

    # 1. sub_bass_ratio: energy 20-80 Hz vs total
    # Use STFT magnitude
    S = np.abs(librosa.stft(y, n_fft=n_fft, hop_length=hop))
    freqs = librosa.fft_frequencies(sr=sr_target, n_fft=n_fft)
    sub_mask = (freqs >= 20) & (freqs <= 80)
    sub_energy = (S[sub_mask, :] ** 2).sum()
    total_energy = (S ** 2).sum() + 1e-12
    sub_bass_ratio = float(sub_energy / total_energy)

    # 2. spectral_flatness: median Wiener entropy across frames
    # librosa returns shape (1, n_frames)
    flatness = librosa.feature.spectral_flatness(y=y, n_fft=n_fft, hop_length=hop)
    spectral_flatness = float(np.median(flatness))

    # 3. attack_rise_time: median onset attack rise time in samples
    # Use onset detection then measure peak/(peak-mean) ratio in a window
    onset_env = librosa.onset.onset_strength(y=y, sr=sr_target, hop_length=hop)
    onset_frames = librosa.onset.onset_detect(
        onset_envelope=onset_env, sr=sr_target, hop_length=hop, backtrack=False
    )
    if len(onset_frames) < 4:
        attack_rise_time = 0.0
    else:
        # For each onset, compute the peak position relative to surrounding window
        rise_times = []
        win = 8  # frames before/after
        for of in onset_frames:
            lo = max(0, of - win)
            hi = min(len(onset_env), of + win + 1)
            window = onset_env[lo:hi]
            if len(window) < 4:
                continue
            peak_idx = window.argmax()
            # rise time = peak position from window start
            rise_times.append(peak_idx)
        attack_rise_time = float(np.median(rise_times)) if rise_times else 0.0

    # 4. mfcc_variance: mean variance of MFCC coefficients across time
    # Synthetic loops -> low variance; live recording -> high variance
    mfcc = librosa.feature.mfcc(y=y, sr=sr_target, n_mfcc=13, n_fft=n_fft, hop_length=hop)
    mfcc_variance = float(np.mean(np.var(mfcc, axis=1)))

    # 5. inharmonicity proxy: spectral contrast variance
    # High contrast variance -> harmonic content varies a lot (acoustic instruments)
    # Low variance -> stable tonal pattern (synth lead/sub)
    contrast = librosa.feature.spectral_contrast(y=y, sr=sr_target, n_fft=n_fft, hop_length=hop)
    contrast_variance = float(np.mean(np.var(contrast, axis=1)))

    return {
        "sub_bass_ratio": sub_bass_ratio,
        "spectral_flatness": spectral_flatness,
        "attack_rise_time": attack_rise_time,
        "mfcc_variance": mfcc_variance,
        "contrast_variance": contrast_variance,
    }


def main():
    if len(sys.argv) < 5:
        print(
            "Usage: extract_synthetic_features.py <baseline.tsv> <audio_root> <layout> <dataset_name>"
        )
        sys.exit(1)

    baseline_path = sys.argv[1]
    audio_root = sys.argv[2]
    layout = sys.argv[3]
    dataset_name = sys.argv[4]

    if layout not in ("flat", "subdir"):
        print("layout must be 'flat' or 'subdir'")
        sys.exit(1)

    # Print header
    print(
        "track_id\tdataset\tgt_bpm\tdet_bpm\tlabel\t"
        "sub_bass_ratio\tspectral_flatness\tattack_rise_time\tmfcc_variance\tcontrast_variance"
    )

    count = 0
    errors = 0
    with open(baseline_path) as f:
        next(f)  # skip header
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

            features = compute_features(audio)
            if features is None:
                errors += 1
                continue

            # Compute label: which of {det, det/2, det*2, det*3} matches gt within 4%?
            candidates = [det_bpm, det_bpm / 2.0, det_bpm * 2.0, det_bpm * 3.0]
            label = -1
            for k, c in enumerate(candidates):
                if c > 0 and abs(c - gt_bpm) / gt_bpm <= 0.04:
                    label = k
                    break

            print(
                f"{track_id}\t{dataset_name}\t{gt_bpm}\t{det_bpm}\t{label}\t"
                f"{features['sub_bass_ratio']:.6f}\t"
                f"{features['spectral_flatness']:.6f}\t"
                f"{features['attack_rise_time']:.4f}\t"
                f"{features['mfcc_variance']:.4f}\t"
                f"{features['contrast_variance']:.4f}",
                flush=True,
            )

            count += 1
            if count % 50 == 0:
                print(f"  ... {count} tracks processed", file=sys.stderr)

    print(f"Done: {count} tracks, {errors} errors", file=sys.stderr)


if __name__ == "__main__":
    main()
