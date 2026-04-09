#!/usr/bin/env python3
"""Merge feature TSVs into a single training table.

Combines:
  - bench/{ds}_features.tsv  (Round 4 judge features: 39 cols)
  - bench/{ds}_phrase.tsv    (Round 5 phrase probe features: 11 cols)

Output: bench/combined_features.tsv with all features joined on track_id
        and a 'dataset' column.

Usage:
  merge_features.py
"""

import pandas as pd


def load_with_dataset(judge_path, phrase_path, dataset_name):
    judge = pd.read_csv(judge_path, sep="\t")
    phrase = pd.read_csv(phrase_path, sep="\t")

    # phrase already has track_id, gt_bpm, det_bpm, label, dataset
    # judge has track_id, gt_bpm, det_bpm, det_conf, ..., label
    # We want to join on track_id and keep only one copy of (gt, det, label).

    # Drop duplicate columns from phrase (keep judge's)
    phrase = phrase.drop(columns=["gt_bpm", "det_bpm", "label"], errors="ignore")

    # Rename dataset column to avoid clash if both have it
    if "dataset" in phrase.columns:
        pass
    else:
        phrase["dataset"] = dataset_name

    merged = judge.merge(phrase, on="track_id", how="inner")
    merged["dataset"] = dataset_name
    return merged


def main():
    gs = load_with_dataset(
        "bench/giantsteps_features.tsv",
        "bench/giantsteps_phrase.tsv",
        "GS",
    )
    bb = load_with_dataset(
        "bench/ballroom_features.tsv",
        "bench/ballroom_phrase.tsv",
        "BB",
    )

    # GTZAN may not be present yet -- include only if both files exist
    import os
    frames = [gs, bb]
    print(f"GS: {len(gs)} rows, {len(gs.columns)} columns")
    print(f"BB: {len(bb)} rows, {len(bb.columns)} columns")

    if os.path.exists("bench/gtzan_features.tsv") and os.path.exists("bench/gtzan_phrase.tsv"):
        gz = load_with_dataset(
            "bench/gtzan_features.tsv",
            "bench/gtzan_phrase.tsv",
            "GZ",
        )
        print(f"GZ: {len(gz)} rows, {len(gz.columns)} columns")
        frames.append(gz)
    else:
        print("(GTZAN files not found, skipping)")

    combined = pd.concat(frames, ignore_index=True)
    print(f"Combined: {len(combined)} rows, {len(combined.columns)} columns")

    out_path = "bench/combined_features.tsv"
    combined.to_csv(out_path, sep="\t", index=False)
    print(f"Wrote {out_path}")


if __name__ == "__main__":
    main()
