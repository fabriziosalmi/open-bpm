#!/usr/bin/env python3
"""Round 5: train judge router with phrase probe features added.

Same logic as Round 4 train_judge_router but reads the merged combined_features.tsv
which includes the phrase probe features (best_shift, prominence, sim_1..sim_32, etc.).

Critical test: cross-dataset generalization. Round 4 produced 0 / -370 cross-dataset.
We want to see if the phrase features push that metric into positive territory.

Usage:
  python3 scripts/train_judge_router_v2.py bench/combined_features.tsv
"""

import sys
import numpy as np
import pandas as pd
from sklearn.linear_model import LogisticRegression
from sklearn.model_selection import StratifiedKFold, cross_val_predict
from sklearn.preprocessing import StandardScaler
from sklearn.metrics import classification_report, confusion_matrix


CANDIDATE_LABELS = {0: "det", 1: "det/2", 2: "det*2", 3: "det*3"}


def main():
    if len(sys.argv) < 2:
        print("Usage: train_judge_router_v2.py <combined_features.tsv>")
        sys.exit(1)

    df = pd.read_csv(sys.argv[1], sep="\t")
    print(f"Loaded {len(df)} rows, {len(df.columns)} columns")

    # Drop unfixable rows
    n_before = len(df)
    df = df[df["label"] != -1].reset_index(drop=True)
    print(f"Dropped {n_before - len(df)} unfixable rows; trainable: {len(df)}")

    # Label distribution
    print("\nLabel distribution:")
    for cls, cnt in df["label"].value_counts().sort_index().items():
        print(f"  {cls} ({CANDIDATE_LABELS.get(cls, '?'):6s}): {cnt:5d} ({cnt*100/len(df):5.1f}%)")

    # Drop identifiers + absolute BPMs (so the classifier doesn't learn dataset shortcuts)
    drop_cols = [
        "track_id", "dataset", "gt_bpm", "label",
        "det_bpm", "ioi_bpm", "comb_bpm", "ac_bpm", "lowac_bpm", "hopf_bpm", "spec_bpm",
        "bpm_librosa",  # also dataset-correlated
        "mode",  # categorical, would need encoding (and it's all "phrase" or "whole")
    ]
    feature_cols = [c for c in df.columns if c not in drop_cols]
    # Drop any non-numeric columns that might still be in
    keep = []
    for c in feature_cols:
        try:
            df[c].astype(float)
            keep.append(c)
        except (ValueError, TypeError):
            print(f"  (dropping non-numeric column: {c})")
    feature_cols = keep
    print(f"\nFeature count: {len(feature_cols)}")

    X = df[feature_cols].values.astype(np.float64)
    y = df["label"].values.astype(np.int64)

    # Replace any NaN with 0
    X = np.nan_to_num(X, nan=0.0, posinf=0.0, neginf=0.0)

    is_gs = (df["dataset"] == "GS").values
    is_bb = (df["dataset"] == "BB").values
    is_gz = (df["dataset"] == "GZ").values

    # === In-distribution: 5-fold stratified cross-validation ===
    scaler = StandardScaler().fit(X)
    Xs = scaler.transform(X)
    skf = StratifiedKFold(n_splits=5, shuffle=True, random_state=42)
    clf = LogisticRegression(solver="lbfgs", max_iter=2000, C=1.0)

    print("\n=== In-distribution: 5-fold CV ===")
    y_pred = cross_val_predict(clf, Xs, y, cv=skf)
    y_proba = cross_val_predict(clf, Xs, y, cv=skf, method="predict_proba")

    print(classification_report(y, y_pred, target_names=[CANDIDATE_LABELS[i] for i in sorted(set(y))]))

    print("=== Confusion (rows=true, cols=pred) ===")
    cm = confusion_matrix(y, y_pred)
    header = "       " + "  ".join(f"{CANDIDATE_LABELS[c]:>6s}" for c in sorted(set(y)))
    print(header)
    for i, row in zip(sorted(set(y)), cm):
        print(f" {CANDIDATE_LABELS[i]:>6s} " + "  ".join(f"{v:6d}" for v in row))

    base = (y == 0).sum()
    n = len(y)
    rout = (y_pred == y).sum()
    print(f"\nBaseline (always det): {base}/{n} ({base*100/n:.1f}%)")
    print(f"Router argmax:         {rout}/{n} ({rout*100/n:.1f}%) delta={rout-base:+d}")

    print("\nPer-dataset (in-distribution CV):")
    dataset_masks = [("GiantSteps", is_gs), ("Ballroom", is_bb), ("GTZAN", is_gz)]
    for name, mask in dataset_masks:
        nm = mask.sum()
        if nm == 0:
            continue
        b = (y[mask] == 0).sum()
        r = (y_pred[mask] == y[mask]).sum()
        print(f"  {name:10s} n={nm:4d}  base={b*100/nm:5.1f}%  router={r*100/nm:5.1f}%  delta={r-b:+4d}")

    # Threshold-gated
    print("\n=== Threshold-gated (override det only if P(non-zero) > t) ===")
    for t in [0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80]:
        gated = np.zeros_like(y)
        for i in range(len(y)):
            non_zero = y_proba[i, 1:]
            if len(non_zero) > 0:
                bk = non_zero.argmax() + 1
                if y_proba[i, bk] > t:
                    gated[i] = bk
        gc = (gated == y).sum()
        line = f"  t={t:.2f}  total={gc:4d}/{n} ({gc*100/n:5.1f}%) delta={gc-base:+4d}  "
        for name, mask in dataset_masks:
            if mask.sum() == 0:
                continue
            db = (y[mask] == 0).sum()
            dr = (gated[mask] == y[mask]).sum()
            line += f"{name[:2]}={dr:3d}/{mask.sum()} ({dr-db:+4d})  "
        print(line)

    # === Cross-dataset (the critical test) ===
    # With 3 datasets, train on 2 and test on the held-out one.
    print("\n=== CROSS-DATASET (held-out dataset, no leakage) ===")
    splits = []
    if is_gz.any():
        splits = [
            ("Train=GS+BB Test=GZ", is_gs | is_bb, is_gz),
            ("Train=GS+GZ Test=BB", is_gs | is_gz, is_bb),
            ("Train=BB+GZ Test=GS", is_bb | is_gz, is_gs),
        ]
    else:
        splits = [
            ("Train=GS Test=BB", is_gs, is_bb),
            ("Train=BB Test=GS", is_bb, is_gs),
        ]

    for train_name, train_mask, test_mask in splits:
        scl = StandardScaler().fit(X[train_mask])
        Xtr = scl.transform(X[train_mask])
        Xte = scl.transform(X[test_mask])
        ytr = y[train_mask]
        yte = y[test_mask]

        clf_x = LogisticRegression(solver="lbfgs", max_iter=2000, C=1.0)
        clf_x.fit(Xtr, ytr)
        proba_te = clf_x.predict_proba(Xte)
        full_proba = np.zeros((len(yte), 4))
        for j, c in enumerate(clf_x.classes_):
            full_proba[:, c] = proba_te[:, j]

        argmax = full_proba.argmax(axis=1)
        base_te = (yte == 0).sum()
        argmax_corr = (argmax == yte).sum()

        # Threshold-gated at t=0.60
        gated = np.zeros_like(yte)
        for i in range(len(yte)):
            nz = full_proba[i, 1:]
            if len(nz) > 0:
                bk = nz.argmax() + 1
                if full_proba[i, bk] > 0.60:
                    gated[i] = bk
        gated_corr = (gated == yte).sum()

        n_te = len(yte)
        print(
            f"  {train_name:25s}  base={base_te:3d}/{n_te}  "
            f"argmax={argmax_corr:3d} ({argmax_corr-base_te:+4d})  "
            f"gated_t60={gated_corr:3d} ({gated_corr-base_te:+4d})"
        )

        # Try multiple thresholds
        for t in [0.50, 0.65, 0.70, 0.75, 0.80, 0.85]:
            g = np.zeros_like(yte)
            for i in range(len(yte)):
                nz = full_proba[i, 1:]
                if len(nz) > 0:
                    bk = nz.argmax() + 1
                    if full_proba[i, bk] > t:
                        g[i] = bk
            gc = (g == yte).sum()
            print(f"    t={t:.2f}: {gc:3d} ({gc-base_te:+4d})")

    # === Final model + top features ===
    print("\n=== Final model on full dataset ===")
    clf.fit(Xs, y)
    print(f"Training accuracy: {clf.score(Xs, y)*100:.1f}%")

    print("\n=== Top 8 features per class (by |coef|) ===")
    coefs = clf.coef_
    for cls_idx, cls in enumerate(clf.classes_):
        top_idx = np.argsort(np.abs(coefs[cls_idx]))[-8:][::-1]
        print(f"\nClass {cls} ({CANDIDATE_LABELS.get(cls, '?')}):")
        for idx in top_idx:
            print(f"  {feature_cols[idx]:25s}  coef={coefs[cls_idx, idx]:+.4f}")


if __name__ == "__main__":
    main()
