#!/bin/bash
# Personal library benchmark for open-bpm
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AUDIO_DIR="$SCRIPT_DIR/test-audio"
GT_FILE="$AUDIO_DIR/ground_truth.tsv"
BINARY="$SCRIPT_DIR/../target/release/open-bpm"

[ ! -f "$BINARY" ] && (cd "$SCRIPT_DIR/.." && cargo build --release 2>/dev/null)

total=0; acc1=0; acc2=0; errors=0

echo -e "track\tgt\tdetected\terror%\tacc1\tgenre"

tail -n +2 "$GT_FILE" | while IFS=$'\t' read -r fname gt_bpm confidence genre; do
    # Find the audio file (fuzzy match on filename prefix)
    audio=$(find "$AUDIO_DIR" -maxdepth 1 -type f \( -name "*.flac" -o -name "*.mp3" -o -name "*.wav" -o -name "*.ogg" \) | grep -F "${fname}" | head -1)
    [ -z "$audio" ] && continue

    detected=$("$BINARY" "$audio" 2>/dev/null | head -1 | awk '{print $1}')
    [ -z "$detected" ] && detected="0"

    error_pct=$(echo "scale=1; ($detected - $gt_bpm) / $gt_bpm * 100" | bc -l 2>/dev/null || echo "?")
    abs_err=$(echo "scale=4; e = ($detected - $gt_bpm) / $gt_bpm; if (e < 0) e = -e; e" | bc -l 2>/dev/null || echo "1")

    status="FAIL"
    if (( $(echo "$abs_err <= 0.04" | bc -l 2>/dev/null) )); then status="PASS"; fi

    # Short name for display
    short=$(echo "$fname" | cut -c1-40)
    echo -e "${short}\t${gt_bpm}\t${detected}\t${error_pct}%\t${status}\t${genre}"
done

echo ""
echo "=== Summary ==="
tail -n +2 "$GT_FILE" | while IFS=$'\t' read -r fname gt_bpm confidence genre; do
    audio=$(find "$AUDIO_DIR" -maxdepth 1 -type f \( -name "*.flac" -o -name "*.mp3" -o -name "*.wav" -o -name "*.ogg" \) | grep -F "${fname}" | head -1)
    [ -z "$audio" ] && continue
    detected=$("$BINARY" "$audio" 2>/dev/null | head -1 | awk '{print $1}')
    [ -z "$detected" ] && continue
    abs_err=$(echo "scale=4; e = ($detected - $gt_bpm) / $gt_bpm; if (e < 0) e = -e; e" | bc -l 2>/dev/null || echo "1")
    if (( $(echo "$abs_err <= 0.04" | bc -l 2>/dev/null) )); then echo "PASS"; else echo "FAIL"; fi
done | awk '{ if ($1=="PASS") p++; total++ } END { printf "Acc1: %d/%d (%.1f%%)\n", p, total, p*100/total }'
