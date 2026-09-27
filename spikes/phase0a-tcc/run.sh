#!/usr/bin/env bash
# Run the Phase 0a spike and measure the result independently of the probe.
#
# The probe reports its own RMS. That is not good enough on its own — the whole
# point of this ticket is to distrust self-reported success — so this script
# re-measures the WAV with ffmpeg, which has no idea what the probe claimed.
#
#   ./run.sh                       # 12s, system audio only
#   ./run.sh --mic --seconds 20    # also exercise the microphone TCC service
#   ./run.sh --reset               # tccutil reset first, to force a fresh prompt
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$HERE/build/meet-ai.app"
BUNDLE_ID="pro.saleschat.meetai"
# Deliberately outside the repo: recordings are private and must never be
# committable. Deliberately not under $HOME or $TMPDIR either — `open` hands the
# app to LaunchServices, which may resolve those differently from this shell, and
# then the script would read one directory while the app writes another.
OUT="${MEET_AI_SPIKE_OUT:-/tmp/meet-ai-phase0a-run}"

SECONDS_ARG=12
MIC=""
RESET=""
NO_TONE=""
EXTRA=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --seconds) SECONDS_ARG="$2"; shift 2 ;;
    --mic) MIC="--mic"; shift ;;
    --no-tone) NO_TONE="--no-tone"; shift ;;
    --tap-autostart) EXTRA="$EXTRA --tap-autostart"; shift ;;
    --reset) RESET="1"; shift ;;
    *) echo "unknown arg: $1" >&2; exit 64 ;;
  esac
done

[[ -d "$APP" ]] || { echo "no bundle at $APP — run ./build.sh first" >&2; exit 1; }

if [[ -n "$RESET" ]]; then
  echo "==> tccutil reset"
  tccutil reset AudioCapture "$BUNDLE_ID" || true
  tccutil reset Microphone "$BUNDLE_ID" || true
fi

rm -rf "$OUT"
mkdir -p "$OUT"
START_EPOCH=$(date +%s)

echo "==> launching $APP (out=$OUT, ${SECONDS_ARG}s)"
# `open` goes through LaunchServices, so the app — not the shell — is the TCC
# responsible process. Launching the helper directly from a terminal would
# attribute the prompt to the terminal and invalidate the whole test.
open -a "$APP" --args --out "$OUT" --seconds "$SECONDS_ARG" $MIC $NO_TONE $EXTRA

DEADLINE=$(( SECONDS_ARG + 90 ))
echo "==> waiting up to ${DEADLINE}s for app-done.json (grant the prompt if one appears)"
for _ in $(seq 1 "$DEADLINE"); do
  [[ -f "$OUT/app-done.json" ]] && break
  sleep 1
done

echo
echo "================ app log ================"
cat "$OUT/run.log" 2>/dev/null || echo "(no run.log — the app never started)"
echo
echo "================ probe log ================"
cat "$OUT/probe.log" 2>/dev/null || echo "(no probe.log)"
echo
echo "================ probe result (self-reported) ================"
cat "$OUT/probe-result.json" 2>/dev/null || echo "(no probe-result.json)"

echo
echo "================ INDEPENDENT measurement (ffmpeg) ================"
for f in system.wav mic.wav; do
  [[ -f "$OUT/$f" ]] || continue
  echo "--- $f ---"
  ffprobe -v error -show_entries stream=codec_name,sample_rate,channels,duration \
          -show_entries format=duration,size -of default=noprint_wrappers=1 "$OUT/$f" || true
  # volumedetect knows nothing about what the probe claimed.
  ffmpeg -v error -nostats -i "$OUT/$f" -af volumedetect -f null - 2>&1 | grep -E "mean_volume|max_volume|histogram_0db" || true
  # And a bluntly literal check: is every byte of the payload zero?
  if tail -c +45 "$OUT/$f" | LC_ALL=C tr -d '\000' | head -c 1 | read -r -n1 _; then
    echo "payload: contains non-zero bytes"
  else
    echo "payload: ALL ZERO BYTES (digital silence)"
  fi
done

echo
echo "================ TCC attribution (system log) ================"
ELAPSED=$(( $(date +%s) - START_EPOCH + 5 ))
log show --last "${ELAPSED}s" --style compact \
    --predicate 'subsystem == "com.apple.TCC" AND composedMessage CONTAINS[c] "meetai"' 2>/dev/null \
  | head -40 || echo "(log query unavailable)"

echo
echo "artifacts in: $OUT"
