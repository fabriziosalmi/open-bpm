#!/usr/bin/env python3
"""Train a logistic-regression judge router for BPM correction.

Reads feature TSV(s) produced by `extract_features`, trains a multinomial
logistic regression to predict which candidate among {det, det/2, det*2, det*3}
matches the ground truth, and reports accuracy and projected pipeline gains.

Usage:
    python3 scripts/train_judge_router.py bench/giantsteps_features.tsv bench/ballroom_features.tsv
"""

import sys
import numpy as np
import pandas as pd
from sklearn.linear_model import LogisticRegression
from sklearn.model_selection import StratifiedKFold, cross_val_predict
from sklearn.preprocessing import StandardScaler
from sklearn.metrics import classification_report, confusion_matrix


CANDIDATE_LABELS = {
    0: "det",
    1: "det/2",
    2: "det*2",
    3: "det*3",
}


def load(paths):
    frames = [pd.read_csv(p, sep="\t") for p in paths]
    df = pd.concat(frames, ignore_index=True)
    return df


def main():
    if len(sys.argv) < 2:
        print("Usage: train_judge_router.py <features.tsv> [<features2.tsv> ...]")
        sys.exit(1)

    df = load(sys.argv[1:])
    print(f"Loaded {len(df)} rows from {len(sys.argv) - 1} file(s)")

    # Drop rows where the gt cannot be reached from any candidate (unfixable)
    unfixable = df[df["label"] == -1]
    print(f"Unfixable rows (gt not in {{det, det/2, det*2, det*3}}): {len(unfixable)} ({len(unfixable)*100/len(df):.1f}%)")
    df = df[df["label"] != -1].reset_index(drop=True)
    print(f"Trainable rows: {len(df)}")

    # Label distribution
    print("\nLabel distribution:")
    for cls, cnt in df["label"].value_counts().sort_index().items():
        print(f"  {cls} ({CANDIDATE_LABELS[cls]:6s}): {cnt:5d} ({cnt*100/len(df):5.1f}%)")

    # Feature columns: drop identifiers, label, and absolute BPM values that
    # would let the classifier identify the dataset (gt_bpm, det_bpm, individual
    # estimator BPMs). Keep only RELATIVE/SHAPE features that should generalize.
    drop_cols = [
        "track_id", "gt_bpm", "label",
        "det_bpm", "ioi_bpm", "comb_bpm", "ac_bpm", "lowac_bpm", "hopf_bpm", "spec_bpm",
    ]
    feature_cols = [c for c in df.columns if c not in drop_cols]
    print(f"\nFeature count: {len(feature_cols)} (absolute BPMs dropped)")

    X = df[feature_cols].values.astype(np.float64)
    y = df["label"].values.astype(np.int64)

    # Standardize features (required for LR with L2)
    scaler = StandardScaler()
    X_scaled = scaler.fit_transform(X)

    # Stratified 5-fold cross-validation.
    # No class_weight: we want HIGH precision on class 0 (don't break what works).
    skf = StratifiedKFold(n_splits=5, shuffle=True, random_state=42)
    clf = LogisticRegression(
        solver="lbfgs",
        max_iter=2000,
        C=1.0,
    )

    print("\nRunning 5-fold stratified cross-validation (vanilla LR)...")
    y_pred = cross_val_predict(clf, X_scaled, y, cv=skf)
    y_proba = cross_val_predict(clf, X_scaled, y, cv=skf, method="predict_proba")

    print("\n=== Classification report ===")
    print(classification_report(y, y_pred, target_names=[CANDIDATE_LABELS[i] for i in sorted(set(y))]))

    print("=== Confusion matrix (rows = true, cols = predicted) ===")
    cm = confusion_matrix(y, y_pred, labels=sorted(set(y)))
    header = "      " + "  ".join(f"{CANDIDATE_LABELS[c]:>6s}" for c in sorted(set(y)))
    print(header)
    for i, row in zip(sorted(set(y)), cm):
        print(f"{CANDIDATE_LABELS[i]:>6s}  " + "  ".join(f"{v:6d}" for v in row))

    # === Projected pipeline impact ===
    # Baseline: how many tracks have label==0 (det was already correct)?
    baseline_correct = (y == 0).sum()
    n_total = len(y)
    print(f"\n=== Pipeline impact (within trainable subset, n={n_total}) ===")
    print(f"Baseline (always trust det):       {baseline_correct}/{n_total} ({baseline_correct*100/n_total:.1f}%)")

    # With router: predict for each track which candidate to use
    router_correct = (y_pred == y).sum()
    print(f"With router (use predicted class, argmax): {router_correct}/{n_total} ({router_correct*100/n_total:.1f}%)")
    delta = router_correct - baseline_correct
    print(f"Net change: {delta:+d} tracks ({delta*100/n_total:+.1f} pp)")

    # Per-source breakdown if we can detect dataset
    is_ballroom = df["track_id"].str.contains("Albums-|Media-")
    print()
    for name, mask in [("GiantSteps", ~is_ballroom), ("Ballroom", is_ballroom)]:
        n = mask.sum()
        if n == 0:
            continue
        base = (y[mask] == 0).sum()
        rout = (y_pred[mask] == y[mask]).sum()
        d = rout - base
        print(f"  {name:10s} n={n:4d}  baseline={base*100/n:5.1f}%  router={rout*100/n:5.1f}%  delta={d:+4d}")

    # === Threshold-gated correction ===
    # Only override det when the router is confident in a NON-zero class.
    # This protects the high-baseline GiantSteps while still correcting Ballroom.
    print("\n=== Threshold-gated router (override det only if P(non-zero) > t) ===")
    for threshold in [0.50, 0.60, 0.70, 0.75, 0.80, 0.85, 0.90]:
        # Default: trust det. Override only if argmax is non-zero AND that prob > t.
        gated = np.zeros_like(y)  # default class 0 = det
        for i in range(len(y)):
            non_zero_probs = y_proba[i, 1:]  # P(class 1), P(class 2), P(class 3)
            best_non_zero = non_zero_probs.argmax() + 1
            if y_proba[i, best_non_zero] > threshold:
                gated[i] = best_non_zero

        gated_correct = (gated == y).sum()
        gd = gated_correct - baseline_correct

        # Per-dataset breakdown
        gs_mask = ~is_ballroom
        bb_mask = is_ballroom
        gs_base = (y[gs_mask] == 0).sum()
        gs_rout = (gated[gs_mask] == y[gs_mask]).sum()
        bb_base = (y[bb_mask] == 0).sum()
        bb_rout = (gated[bb_mask] == y[bb_mask]).sum()

        print(
            f"  t={threshold:.2f}  total={gated_correct:4d}/{n_total} ({gated_correct*100/n_total:5.1f}%) delta={gd:+4d}  "
            f"GS={gs_rout:3d}/{gs_mask.sum()} ({(gs_rout-gs_base):+3d})  "
            f"BB={bb_rout:3d}/{bb_mask.sum()} ({(bb_rout-bb_base):+3d})"
        )

    # === Cross-dataset generalization test ===
    # Train on one dataset, test on the other. This is the most honest
    # measure of whether the router learns a transferable pattern.
    print("\n=== Cross-dataset generalization (no leakage) ===")
    is_ballroom_arr = is_ballroom.values
    for train_name, train_mask in [
        ("Train=GiantSteps Test=Ballroom", ~is_ballroom_arr),
        ("Train=Ballroom    Test=GiantSteps", is_ballroom_arr),
    ]:
        test_mask = ~train_mask
        if train_mask.sum() == 0 or test_mask.sum() == 0:
            continue

        scaler_x = StandardScaler().fit(X[train_mask])
        Xtr = scaler_x.transform(X[train_mask])
        Xte = scaler_x.transform(X[test_mask])
        ytr = y[train_mask]
        yte = y[test_mask]

        clf_x = LogisticRegression(solver="lbfgs", max_iter=2000, C=1.0)
        clf_x.fit(Xtr, ytr)
        proba_te = clf_x.predict_proba(Xte)
        # Map probabilities back into the full 4-class space
        full_proba = np.zeros((len(yte), 4))
        for j, cls in enumerate(clf_x.classes_):
            full_proba[:, cls] = proba_te[:, j]

        # Argmax baseline
        pred_argmax = full_proba.argmax(axis=1)
        baseline_te = (yte == 0).sum()
        argmax_te = (pred_argmax == yte).sum()

        # Threshold-gated at t=0.60
        gated = np.zeros_like(yte)
        for i in range(len(yte)):
            non_zero = full_proba[i, 1:]
            if len(non_zero) > 0:
                best = non_zero.argmax() + 1
                if full_proba[i, best] > 0.60:
                    gated[i] = best
        gated_te = (gated == yte).sum()

        n_te = len(yte)
        print(
            f"  {train_name:35s}  base={baseline_te:3d}/{n_te} "
            f"argmax={argmax_te:3d} ({argmax_te-baseline_te:+3d})  "
            f"gated={gated_te:3d} ({gated_te-baseline_te:+3d})"
        )

    # Train final model on all data and report top features
    print("\n=== Training final model on full dataset ===")
    clf.fit(X_scaled, y)
    print(f"Final model accuracy on training set: {clf.score(X_scaled, y)*100:.1f}%")

    # Top contributing features per class
    print("\n=== Top 5 features by absolute coefficient (per class) ===")
    coefs = clf.coef_  # shape: (n_classes, n_features)
    for cls_idx, cls in enumerate(clf.classes_):
        top_idx = np.argsort(np.abs(coefs[cls_idx]))[-5:][::-1]
        print(f"\nClass {cls} ({CANDIDATE_LABELS[cls]}):")
        for idx in top_idx:
            print(f"  {feature_cols[idx]:25s}  coef={coefs[cls_idx, idx]:+.4f}")


if __name__ == "__main__":
    main()
