#!/usr/bin/env python3
"""Compare open-bpm vs librosa on the personal library."""

import csv
import os
import subprocess
import time
import librosa
import numpy as np

AUDIO_DIR = os.path.join(os.path.dirname(__file__), "test-audio")
GT_FILE = os.path.join(AUDIO_DIR, "ground_truth.tsv")
OPENBPM = os.path.join(os.path.dirname(__file__), "..", "target", "release", "open-bpm")

def find_audio(fname):
    """Find audio file matching the ground truth filename."""
    for ext in [".flac", ".mp3", ".wav", ".ogg"]:
        for f in os.listdir(AUDIO_DIR):
            if f.startswith(fname[:30]) and f.endswith(ext):
                return os.path.join(AUDIO_DIR, f)
    return None

def detect_librosa(path):
    """Detect BPM using librosa."""
    try:
        y, sr = librosa.load(path, sr=22050, mono=True)
        onset_env = librosa.onset.onset_strength(y=y, sr=sr)
        # librosa.beat.tempo returns array of tempos
        tempo = librosa.beat.tempo(onset_envelope=onset_env, sr=sr)
        return float(tempo[0])
    except Exception as e:
        return 0.0

def detect_openbpm(path):
    """Detect BPM using open-bpm CLI."""
    try:
        result = subprocess.run([OPENBPM, path], capture_output=True, text=True, timeout=30)
        bpm_str = result.stdout.strip().split()[0]
        return float(bpm_str)
    except:
        return 0.0

def acc1(detected, gt):
    """Within 4% tolerance."""
    if gt <= 0 or detected <= 0:
        return False
    return abs(detected - gt) / gt < 0.04

def acc2(detected, gt):
    """Within 4% of gt, 2*gt, gt/2, 3*gt, gt/3."""
    if gt <= 0 or detected <= 0:
        return False
    for mult in [1, 2, 0.5, 3, 1/3]:
        ref = gt * mult
        if abs(detected - ref) / ref < 0.04:
            return True
    return False

# Load ground truth
tracks = []
with open(GT_FILE) as f:
    reader = csv.DictReader(f, delimiter='\t')
    for row in reader:
        tracks.append(row)

print(f"{'Track':<42} {'GT':>5} {'open-bpm':>9} {'librosa':>9} {'OB':>4} {'LR':>4}")
print("-" * 80)

ob_correct = 0
lr_correct = 0
ob_acc2 = 0
lr_acc2 = 0
ob_time_total = 0
lr_time_total = 0
total = 0

for track in tracks:
    fname = track['filename']
    gt = float(track['bpm'])

    audio = find_audio(fname)
    if not audio:
        continue

    # open-bpm
    t0 = time.time()
    ob_bpm = detect_openbpm(audio)
    ob_time = time.time() - t0
    ob_time_total += ob_time

    # librosa
    t0 = time.time()
    lr_bpm = detect_librosa(audio)
    lr_time = time.time() - t0
    lr_time_total += lr_time

    ob_ok = "PASS" if acc1(ob_bpm, gt) else "FAIL"
    lr_ok = "PASS" if acc1(lr_bpm, gt) else "FAIL"

    if acc1(ob_bpm, gt): ob_correct += 1
    if acc1(lr_bpm, gt): lr_correct += 1
    if acc2(ob_bpm, gt): ob_acc2 += 1
    if acc2(lr_bpm, gt): lr_acc2 += 1
    total += 1

    short = fname[:40]
    print(f"{short:<42} {gt:>5} {ob_bpm:>9.2f} {lr_bpm:>9.2f} {ob_ok:>4} {lr_ok:>4}")

print("-" * 80)
print(f"\n{'RESULTS':=^80}")
print(f"")
print(f"  {'Metric':<25} {'open-bpm':>12} {'librosa':>12}")
print(f"  {'-'*25} {'-'*12} {'-'*12}")
print(f"  {'Acc1 (4% tolerance)':<25} {ob_correct:>5}/{total} ({100*ob_correct/total:>4.1f}%) {lr_correct:>4}/{total} ({100*lr_correct/total:>4.1f}%)")
print(f"  {'Acc2 (octave-tolerant)':<25} {ob_acc2:>5}/{total} ({100*ob_acc2/total:>4.1f}%) {lr_acc2:>4}/{total} ({100*lr_acc2/total:>4.1f}%)")
print(f"  {'Total time':<25} {ob_time_total:>9.1f}s   {lr_time_total:>9.1f}s")
print(f"  {'Avg per track':<25} {ob_time_total/total:>9.2f}s   {lr_time_total/total:>9.2f}s")
print()

# Show where they differ
print(f"{'DISAGREEMENTS':=^80}")
for track in tracks:
    fname = track['filename']
    gt = float(track['bpm'])
    audio = find_audio(fname)
    if not audio:
        continue
    ob_bpm = detect_openbpm(audio)
    lr_bpm = detect_librosa(audio)
    ob_ok = acc1(ob_bpm, gt)
    lr_ok = acc1(lr_bpm, gt)
    if ob_ok != lr_ok:
        winner = "open-bpm" if ob_ok else "librosa"
        short = fname[:35]
        print(f"  {short:<37} gt={gt:<6} ob={ob_bpm:<8.2f} lr={lr_bpm:<8.2f} → {winner} wins")
