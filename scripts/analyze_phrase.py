#!/usr/bin/env python3
"""Analyze phrase probe results: does it discriminate intra-dataset label=0 vs others?

Key tests:
  1. best_shift distribution per label, per dataset
  2. prominence discrimination (label=0 vs label=1) within each dataset
  3. is_ternary_top recall on label=1 (should be high if it captures waltzes)
  4. is_edm_phrase_top recall on label=0 (should be high if it captures EDM)

Usage:
  analyze_phrase.py bench/giantsteps_phrase.tsv bench/ballroom_phrase.tsv
"""

import sys
import numpy as np
import pandas as pd


def main():
    if len(sys.argv) < 2:
        print("Usage: analyze_phrase.py <phrase.tsv> [<phrase2.tsv> ...]")
        sys.exit(1)

    frames = [pd.read_csv(p, sep="\t") for p in sys.argv[1:]]
    df = pd.concat(frames, ignore_index=True)
    print(f"Loaded {len(df)} tracks total")

    # Drop unfixable label=-1 for the analysis
    df_lbl = df[df["label"] != -1].copy()
    print(f"After dropping label=-1: {len(df_lbl)}")

    # === Distribution of best_shift per dataset and label ===
    print("\n=== best_shift distribution ===")
    for ds in ["GS", "BB"]:
        sub = df_lbl[df_lbl["dataset"] == ds]
        if len(sub) == 0:
            continue
        print(f"\n--- {ds} (n={len(sub)}) ---")
        for lbl in sorted(sub["label"].unique()):
            s = sub[sub["label"] == lbl]
            counts = s["best_shift"].value_counts().sort_index()
            total = len(s)
            print(f"  label={int(lbl)} (n={total:4d}):")
            for shift in [1, 2, 3, 4, 6, 8, 12, 16, 32]:
                c = counts.get(shift, 0)
                pct = c * 100 / total if total > 0 else 0
                bar = "#" * int(pct / 2)
                print(f"    shift={shift:3d}: {c:4d} ({pct:5.1f}%) {bar}")

    # === Prominence discrimination ===
    print("\n=== Prominence (best_score - median(others)) ===")
    print(f"  {'Dataset':<10} {'Label':<8} {'n':<6} {'mean':<10} {'median':<10} {'std':<10}")
    for ds in ["GS", "BB"]:
        sub = df_lbl[df_lbl["dataset"] == ds]
        for lbl in sorted(sub["label"].unique()):
            s = sub[sub["label"] == lbl]
            if len(s) == 0:
                continue
            print(
                f"  {ds:<10} label={int(lbl):<3d} {len(s):<6d} "
                f"{s['prominence'].mean():<10.4f} {s['prominence'].median():<10.4f} {s['prominence'].std():<10.4f}"
            )

    # === Cohen's d (label=0 vs label=1 within each dataset) for prominence ===
    print("\n=== Cohen's d: label=0 vs label=1 (prominence) ===")
    for ds in ["GS", "BB"]:
        sub = df_lbl[df_lbl["dataset"] == ds]
        a = sub[sub["label"] == 0]["prominence"].values
        b = sub[sub["label"] == 1]["prominence"].values
        if len(a) == 0 or len(b) == 0:
            continue
        pooled_std = np.sqrt((np.var(a) + np.var(b)) / 2.0)
        d = (np.mean(a) - np.mean(b)) / pooled_std if pooled_std > 1e-12 else 0.0
        print(f"  {ds}: n0={len(a):4d} n1={len(b):4d}  d={d:+.4f}")

    # === is_ternary_top: precision/recall as a label=1 detector ===
    print("\n=== is_ternary_top as label=1 (halve) detector ===")
    for ds in ["GS", "BB"]:
        sub = df_lbl[df_lbl["dataset"] == ds]
        if len(sub) == 0:
            continue
        # Treat is_ternary_top=1 as "predict halve"
        tp = ((sub["is_ternary_top"] == 1) & (sub["label"] == 1)).sum()
        fp = ((sub["is_ternary_top"] == 1) & (sub["label"] != 1)).sum()
        fn = ((sub["is_ternary_top"] == 0) & (sub["label"] == 1)).sum()
        tn = ((sub["is_ternary_top"] == 0) & (sub["label"] != 1)).sum()
        precision = tp / (tp + fp) if (tp + fp) > 0 else 0
        recall = tp / (tp + fn) if (tp + fn) > 0 else 0
        print(f"  {ds}: TP={tp:3d} FP={fp:3d} FN={fn:3d} TN={tn:3d}  precision={precision:.3f} recall={recall:.3f}")

    # === is_edm_phrase_top: precision/recall as a label=0 (keep) detector ===
    print("\n=== is_edm_phrase_top as label=0 (keep) detector ===")
    for ds in ["GS", "BB"]:
        sub = df_lbl[df_lbl["dataset"] == ds]
        if len(sub) == 0:
            continue
        tp = ((sub["is_edm_phrase_top"] == 1) & (sub["label"] == 0)).sum()
        fp = ((sub["is_edm_phrase_top"] == 1) & (sub["label"] != 0)).sum()
        fn = ((sub["is_edm_phrase_top"] == 0) & (sub["label"] == 0)).sum()
        tn = ((sub["is_edm_phrase_top"] == 0) & (sub["label"] != 0)).sum()
        precision = tp / (tp + fp) if (tp + fp) > 0 else 0
        recall = tp / (tp + fn) if (tp + fn) > 0 else 0
        print(f"  {ds}: TP={tp:3d} FP={fp:3d} FN={fn:3d} TN={tn:3d}  precision={precision:.3f} recall={recall:.3f}")

    # === Cross-dataset prominence distribution ===
    print("\n=== Prominence distribution (dataset-level proxy?) ===")
    for ds in ["GS", "BB"]:
        sub = df_lbl[df_lbl["dataset"] == ds]
        print(f"  {ds}: median={sub['prominence'].median():.4f} mean={sub['prominence'].mean():.4f} std={sub['prominence'].std():.4f}")


if __name__ == "__main__":
    main()
