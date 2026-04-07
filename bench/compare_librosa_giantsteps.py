#!/usr/bin/env python3
"""Compare open-bpm vs librosa on the full GiantSteps dataset (664 tracks)."""

import os
import subprocess
import sys
import time
import librosa
import numpy as np

ANNO_DIR = os.path.join(os.path.dirname(__file__), "giantsteps-tempo-dataset", "annotations", "tempo")
AUDIO_DIR = os.path.join(os.path.dirname(__file__), "giantsteps-audio")
OPENBPM = os.path.join(os.path.dirname(__file__), "..", "target", "release", "open-bpm")

def detect_librosa(path):
    try:
        y, sr = librosa.load(path, sr=22050, mono=True)
        onset_env = librosa.onset.onset_strength(y=y, sr=sr)
        tempo = librosa.beat.tempo(onset_envelope=onset_env, sr=sr)
        return float(tempo[0])
    except:
        return 0.0

def detect_openbpm(path):
    try:
        result = subprocess.run([OPENBPM, path], capture_output=True, text=True, timeout=30)
        return float(result.stdout.strip().split()[0])
    except:
        return 0.0

def acc1(det, gt):
    return gt > 0 and det > 0 and abs(det - gt) / gt < 0.04

def acc2(det, gt):
    if gt <= 0 or det <= 0:
        return False
    for m in [1, 2, 0.5, 3, 1/3]:
        if abs(det - gt * m) / (gt * m) < 0.04:
            return True
    return False

# Collect all tracks
annos = sorted(os.listdir(ANNO_DIR))
total = len(annos)

ob_a1 = ob_a2 = lr_a1 = lr_a2 = 0
ob_time = lr_time = 0.0
count = 0
ob_wins = lr_wins = both_right = both_wrong = 0

for i, anno_file in enumerate(annos):
    track_id = anno_file.replace(".bpm", "")
    audio = os.path.join(AUDIO_DIR, f"{track_id}.mp3")
    if not os.path.exists(audio):
        continue

    gt = float(open(os.path.join(ANNO_DIR, anno_file)).read().strip())
    if gt <= 0:
        continue

    t0 = time.time()
    ob = detect_openbpm(audio)
    ob_time += time.time() - t0

    t0 = time.time()
    lr = detect_librosa(audio)
    lr_time += time.time() - t0

    count += 1
    ob_ok = acc1(ob, gt)
    lr_ok = acc1(lr, gt)
    if ob_ok: ob_a1 += 1
    if lr_ok: lr_a1 += 1
    if acc2(ob, gt): ob_a2 += 1
    if acc2(lr, gt): lr_a2 += 1

    if ob_ok and not lr_ok: ob_wins += 1
    elif lr_ok and not ob_ok: lr_wins += 1
    elif ob_ok and lr_ok: both_right += 1
    else: both_wrong += 1

    if (i + 1) % 50 == 0:
        print(f"  ... {i+1}/{total} tracks processed", file=sys.stderr)

print(f"")
print(f"{'=' * 60}")
print(f"  GiantSteps Tempo — open-bpm vs librosa ({count} tracks)")
print(f"{'=' * 60}")
print(f"")
print(f"  {'Metric':<25} {'open-bpm':>12} {'librosa':>12}")
print(f"  {'-'*25} {'-'*12} {'-'*12}")
print(f"  {'Acc1 (4%)':<25} {ob_a1:>5}/{count} ({100*ob_a1/count:>4.1f}%) {lr_a1:>4}/{count} ({100*lr_a1/count:>4.1f}%)")
print(f"  {'Acc2 (octave)':<25} {ob_a2:>5}/{count} ({100*ob_a2/count:>4.1f}%) {lr_a2:>4}/{count} ({100*lr_a2/count:>4.1f}%)")
print(f"  {'Total time':<25} {ob_time:>9.1f}s   {lr_time:>9.1f}s")
print(f"  {'Avg per track':<25} {ob_time/count:>9.2f}s   {lr_time/count:>9.2f}s")
print(f"")
print(f"  Head-to-head:")
print(f"    open-bpm wins:  {ob_wins}")
print(f"    librosa wins:   {lr_wins}")
print(f"    Both correct:   {both_right}")
print(f"    Both wrong:     {both_wrong}")
print(f"{'=' * 60}")
