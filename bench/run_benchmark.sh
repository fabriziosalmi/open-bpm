#!/bin/bash
# GiantSteps Tempo Benchmark for open-bpm
#
# Metrics (standard MIR tempo evaluation):
#   Acc1: |detected - truth| <= 4% of truth
#   Acc2: same as Acc1 but also allows 2x, 0.5x, 3x, 1/3x (octave-tolerant)
#
# Usage:
#   ./bench/run_benchmark.sh [N]    (N = max tracks, default all)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ANNO_DIR="$SCRIPT_DIR/giantsteps-tempo-dataset/annotations/tempo"
AUDIO_DIR="$SCRIPT_DIR/giantsteps-audio"
BINARY="$SCRIPT_DIR/../target/release/open-bpm"
MAX_TRACKS="${1:-9999}"

# Build release if needed
if [ ! -f "$BINARY" ]; then
    echo "Building release..."
    (cd "$SCRIPT_DIR/.." && cargo build --release 2>/dev/null)
fi

# Counters
total=0
acc1_pass=0
acc2_pass=0
errors=0
total_error=0.0
total_abs_error=0.0
octave_errors=0
missing=0

# Results file
RESULTS="$SCRIPT_DIR/benchmark_results.tsv"
echo -e "track_id\tground_truth\tdetected\terror_pct\tacc1\tacc2" > "$RESULTS"

for anno in "$ANNO_DIR"/*.bpm; do
    [ "$total" -ge "$MAX_TRACKS" ] && break

    basename=$(basename "$anno" .bpm)
    audio="$AUDIO_DIR/${basename}.mp3"

    if [ ! -f "$audio" ]; then
        missing=$((missing + 1))
        continue
    fi

    # Read ground truth BPM
    gt_bpm=$(cat "$anno" | tr -d '[:space:]')
    if [ -z "$gt_bpm" ] || [ "$gt_bpm" = "0" ]; then
        continue
    fi

    # Run detector
    detected=$("$BINARY" "$audio" 2>/dev/null | head -1 | awk '{print $1}')
    if [ -z "$detected" ] || [ "$detected" = "0.00" ]; then
        errors=$((errors + 1))
        echo -e "${basename}\t${gt_bpm}\t0\t-\tFAIL\tFAIL" >> "$RESULTS"
        total=$((total + 1))
        continue
    fi

    # Compute error percentage
    error_pct=$(echo "scale=4; ($detected - $gt_bpm) / $gt_bpm * 100" | bc -l 2>/dev/null || echo "0")
    abs_error_pct=$(echo "scale=4; e = $error_pct; if (e < 0) e = -e; e" | bc -l 2>/dev/null || echo "0")

    # Acc1: within 4%
    acc1="FAIL"
    if (( $(echo "$abs_error_pct <= 4.0" | bc -l) )); then
        acc1="PASS"
        acc1_pass=$((acc1_pass + 1))
    fi

    # Acc2: within 4% of truth, 2*truth, truth/2, 3*truth, truth/3
    acc2="FAIL"
    for mult in 1.0 2.0 0.5 3.0 0.3333; do
        ref=$(echo "scale=4; $gt_bpm * $mult" | bc -l)
        ref_error=$(echo "scale=4; e = ($detected - $ref) / $ref * 100; if (e < 0) e = -e; e" | bc -l 2>/dev/null || echo "999")
        if (( $(echo "$ref_error <= 4.0" | bc -l) )); then
            acc2="PASS"
            break
        fi
    done
    if [ "$acc2" = "PASS" ]; then
        acc2_pass=$((acc2_pass + 1))
    fi

    # Track octave errors (Acc2 pass but Acc1 fail = octave error)
    if [ "$acc2" = "PASS" ] && [ "$acc1" = "FAIL" ]; then
        octave_errors=$((octave_errors + 1))
    fi

    echo -e "${basename}\t${gt_bpm}\t${detected}\t${error_pct}\t${acc1}\t${acc2}" >> "$RESULTS"
    total=$((total + 1))

    # Progress every 50 tracks
    if [ $((total % 50)) -eq 0 ]; then
        echo "  ... $total tracks processed"
    fi
done

echo ""
echo "=========================================="
echo "  GiantSteps Tempo Benchmark Results"
echo "=========================================="
echo ""
echo "  Tracks tested:    $total"
echo "  Missing audio:    $missing"
echo "  Detection errors: $errors"
echo ""
echo "  Acc1 (4% tol):    $acc1_pass / $total  ($(echo "scale=1; $acc1_pass * 100 / $total" | bc)%)"
echo "  Acc2 (octave):    $acc2_pass / $total  ($(echo "scale=1; $acc2_pass * 100 / $total" | bc)%)"
echo "  Octave errors:    $octave_errors"
echo ""
echo "  Results: $RESULTS"
echo "=========================================="
