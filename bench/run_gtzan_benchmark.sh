#!/bin/bash
# GTZAN Tempo Benchmark for open-bpm
#
# Dataset: 999 tracks (1 corrupted reggae file excluded), ~30s each, 10 genres.
# Audio:    /genres/<genre>/<genre>.<00000>.wav
# Tempo:    /annotations_tempo/gtzan_<genre>_<00000>.bpm
# Note the naming difference: the audio uses dots, the annotations use underscores
# AND the prefix "gtzan_". The track_id we expose is gtzan_<genre>_<00000>.
#
# Metrics (standard MIR tempo evaluation):
#   Acc1: |detected - truth| <= 4% of truth
#   Acc2: same as Acc1 but also allows 2x, 0.5x, 3x, 1/3x (octave-tolerant)
#
# Usage:
#   ./bench/run_gtzan_benchmark.sh [N]    (N = max tracks, default all)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GTZAN_DIR="$HOME/mir_datasets/gtzan_genre"
ANNO_DIR="$GTZAN_DIR/annotations_tempo"
AUDIO_DIR="$GTZAN_DIR/genres"
BINARY="$SCRIPT_DIR/../target/release/open-bpm"
MAX_TRACKS="${1:-9999}"

if [ ! -f "$BINARY" ]; then
    echo "Building release..."
    (cd "$SCRIPT_DIR/.." && cargo build --release 2>/dev/null)
fi

total=0
acc1_pass=0
acc2_pass=0
errors=0
octave_errors=0
missing=0

RESULTS="$SCRIPT_DIR/gtzan_results.tsv"
echo -e "track_id\tground_truth\tdetected\terror_pct\tacc1\tacc2" > "$RESULTS"

# Iterate over annotation files (flat list, easier to map back to canonical id)
while IFS= read -r anno; do
    [ "$total" -ge "$MAX_TRACKS" ] && break

    # Annotation basename: gtzan_<genre>_<00000>
    anno_base=$(basename "$anno" .bpm)
    track_id="$anno_base"

    # Derive audio path: extract genre and number
    # gtzan_blues_00000  -> genre=blues, num=00000 -> /genres/blues/blues.00000.wav
    rest="${anno_base#gtzan_}"
    genre="${rest%_*}"
    num="${rest##*_}"
    audio="$AUDIO_DIR/$genre/$genre.$num.wav"

    if [ ! -f "$audio" ]; then
        missing=$((missing + 1))
        continue
    fi

    # Annotations are in scientific notation (e.g. 1.258700000000000045e+02).
    # Convert to plain decimal that bc can handle.
    gt_raw=$(cat "$anno" | tr -d '[:space:]')
    if [ -z "$gt_raw" ] || [ "$gt_raw" = "0" ]; then
        continue
    fi
    gt_bpm=$(awk -v v="$gt_raw" 'BEGIN { printf "%.4f", v }')

    detected=$("$BINARY" "$audio" 2>/dev/null | head -1 | awk '{print $1}')
    if [ -z "$detected" ] || [ "$detected" = "0.00" ]; then
        errors=$((errors + 1))
        echo -e "${track_id}\t${gt_bpm}\t0\t-\tFAIL\tFAIL" >> "$RESULTS"
        total=$((total + 1))
        continue
    fi

    error_pct=$(echo "scale=4; ($detected - $gt_bpm) / $gt_bpm * 100" | bc -l 2>/dev/null || echo "0")
    abs_error_pct=$(echo "scale=4; e = $error_pct; if (e < 0) e = -e; e" | bc -l 2>/dev/null || echo "0")

    acc1="FAIL"
    if (( $(echo "$abs_error_pct <= 4.0" | bc -l) )); then
        acc1="PASS"
        acc1_pass=$((acc1_pass + 1))
    fi

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

    if [ "$acc2" = "PASS" ] && [ "$acc1" = "FAIL" ]; then
        octave_errors=$((octave_errors + 1))
    fi

    echo -e "${track_id}\t${gt_bpm}\t${detected}\t${error_pct}\t${acc1}\t${acc2}" >> "$RESULTS"
    total=$((total + 1))

    if [ $((total % 50)) -eq 0 ]; then
        echo "  ... $total tracks processed"
    fi
done < <(find "$ANNO_DIR" -name "*.bpm" | sort)

echo ""
echo "=========================================="
echo "  GTZAN Tempo Benchmark Results"
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
