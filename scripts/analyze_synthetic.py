#!/usr/bin/env python3
"""Analyze synthetic-ness features: do they discriminate per dataset and per label?

Usage:
  analyze_synthetic.py bench/giantsteps_synth.tsv bench/ballroom_synth.tsv
"""

import sys
import numpy as np
import pandas as pd

FEATURES = [
    "sub_bass_ratio",
    "spectral_flatness",
    "attack_rise_time",
    "mfcc_variance",
    "contrast_variance",
]


def summary(label, df, features):
    print(f"  n={len(df):4d}", end="")
    for f in features:
        v = df[f].values
        print(f"  {f}=median={np.median(v):8.4f} iqr=[{np.percentile(v, 25):.4f},{np.percentile(v, 75):.4f}]", end="")
    print()


def main():
    if len(sys.argv) < 2:
        print("Usage: analyze_synthetic.py <synth.tsv> [<synth2.tsv> ...]")
        sys.exit(1)

    frames = [pd.read_csv(p, sep="\t") for p in sys.argv[1:]]
    df = pd.concat(frames, ignore_index=True)
    print(f"Loaded {len(df)} tracks total")

    # Drop rows where features are NaN
    df = df.dropna(subset=FEATURES).reset_index(drop=True)
    print(f"After dropna: {len(df)} tracks")

    # === Per-dataset distribution ===
    print("\n=== Feature distribution per DATASET ===")
    print(f"{'Feature':<25}  {'GS median':<12}  {'BB median':<12}  {'GS mean':<10}  {'BB mean':<10}  {'separation':<10}")
    print("-" * 95)
    for f in FEATURES:
        gs = df[df["dataset"] == "GS"][f].values
        bb = df[df["dataset"] == "BB"][f].values
        gs_med = np.median(gs)
        bb_med = np.median(bb)
        gs_mean = np.mean(gs)
        bb_mean = np.mean(bb)
        # Separation: difference of means / pooled std
        pooled_std = np.sqrt((np.var(gs) + np.var(bb)) / 2.0)
        sep = abs(gs_mean - bb_mean) / pooled_std if pooled_std > 1e-12 else 0.0
        print(f"{f:<25}  {gs_med:<12.4f}  {bb_med:<12.4f}  {gs_mean:<10.4f}  {bb_mean:<10.4f}  d={sep:.3f}")

    # === Per-label distribution within each dataset ===
    print("\n=== Feature medians per LABEL within each dataset ===")
    for ds in ["GS", "BB"]:
        sub = df[(df["dataset"] == ds) & (df["label"] != -1)]
        if len(sub) == 0:
            continue
        print(f"\n--- {ds} (n={len(sub)}) ---")
        print(f"  {'label':<8}", end="")
        for f in FEATURES:
            print(f"  {f:<22}", end="")
        print()
        for lbl in sorted(sub["label"].unique()):
            s = sub[sub["label"] == lbl]
            print(f"  {int(lbl):<8} (n={len(s):4d})", end="")
            for f in FEATURES:
                print(f"  {np.median(s[f].values):<22.4f}", end="")
            print()

    # === Synthetic-ness composite score ===
    # Hypothesis: high sub_bass_ratio + low mfcc_variance + low contrast_variance = synthetic
    # Use simple z-score normalization across all data and combine
    print("\n=== Composite synthetic-ness score ===")
    print("Score = z(sub_bass_ratio) - z(mfcc_variance) - z(contrast_variance)")
    print("(higher = more synthetic)")

    z_sb = (df["sub_bass_ratio"] - df["sub_bass_ratio"].mean()) / df["sub_bass_ratio"].std()
    z_mv = (df["mfcc_variance"] - df["mfcc_variance"].mean()) / df["mfcc_variance"].std()
    z_cv = (df["contrast_variance"] - df["contrast_variance"].mean()) / df["contrast_variance"].std()
    df["synth_score"] = z_sb - z_mv - z_cv

    print(f"\n  GS synth_score: median={df[df['dataset']=='GS']['synth_score'].median():.3f}  mean={df[df['dataset']=='GS']['synth_score'].mean():.3f}")
    print(f"  BB synth_score: median={df[df['dataset']=='BB']['synth_score'].median():.3f}  mean={df[df['dataset']=='BB']['synth_score'].mean():.3f}")

    # AUC-ish: how well does the score separate GS from BB?
    gs_scores = df[df["dataset"] == "GS"]["synth_score"].values
    bb_scores = df[df["dataset"] == "BB"]["synth_score"].values
    # Mann-Whitney AUC
    n_gs = len(gs_scores)
    n_bb = len(bb_scores)
    rank = np.argsort(np.concatenate([gs_scores, bb_scores]))
    rank_gs_count = 0
    for i, r in enumerate(rank):
        if r < n_gs:
            rank_gs_count += i + 1
    u_gs = rank_gs_count - n_gs * (n_gs + 1) / 2
    auc = u_gs / (n_gs * n_bb)
    print(f"  AUC (synth_score: GS > BB): {auc:.3f}  (0.5 = no separation, 1.0 = perfect)")

    # === The KEY test: does synth_score correlate with label within each dataset? ===
    print("\n=== synth_score vs label (within each dataset) ===")
    for ds in ["GS", "BB"]:
        sub = df[(df["dataset"] == ds) & (df["label"] != -1)]
        if len(sub) == 0:
            continue
        print(f"\n  {ds}:")
        for lbl in sorted(sub["label"].unique()):
            s = sub[sub["label"] == lbl]
            if len(s) == 0:
                continue
            print(
                f"    label={int(lbl)} (n={len(s):4d}): "
                f"synth_score median={s['synth_score'].median():+.3f}  "
                f"mean={s['synth_score'].mean():+.3f}"
            )


if __name__ == "__main__":
    main()
