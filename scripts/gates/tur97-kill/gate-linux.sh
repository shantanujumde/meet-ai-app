#!/usr/bin/env bash
# TUR-97 gate on Linux (TUR-50): does a recording survive `kill -9` of the
# installed app mid-meeting?
#
# The same idea as gate.sh (macOS): start a real recording in the installed
# app, let it run, `kill -9` it, relaunch it, then check the files a user
# would open: both WAV headers, segments.json, drift-check, and that the app
# starts again with no ERROR or panic in its log.
#
#   scripts/gates/tur97-kill/gate-linux.sh                  # 60 s, kill -9
#   scripts/gates/tur97-kill/gate-linux.sh --seconds 3600   # the 1-hour variant
#   scripts/gates/tur97-kill/gate-linux.sh --app ./target/release/meet-ai
#
# What differs from gate.sh:
# - The recording starts with `meet-ai --toggle-recording` (TUR-58), which
#   reaches the running app through its single-instance guard. No keystroke,
#   so no Accessibility-style permission and it works under Wayland too.
# - There is no meet-stt off macOS, so check 7 (system.wav transcribes) is
#   skipped. `--play FILE` plays a WAV through the default output while
#   recording (pw-play, paplay or aplay) so system.wav is not silent.
# - The relaunched app is ended with SIGTERM (there is no AppleScript Quit);
#   how long that takes is reported, not graded.
#
# Needs: python3, cargo (drift-check is built from this checkout), and a
# desktop session the app can open a window in. See README.md next to this
# file for what every check proves.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
CHECK="$HERE/check.py"

# Where the .deb puts the program. Measure it on a real install; pass --app
# for another one.
APP="/usr/bin/meet-ai"
REC_SECONDS=60
OUT=""
KEEP=""
PLAY=""

usage() { sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) APP="$2"; shift 2 ;;
    --seconds) REC_SECONDS="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --play) PLAY="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1 (see --help)" >&2; exit 64 ;;
  esac
done

[[ "$REC_SECONDS" =~ ^[0-9]+$ ]] || { echo "--seconds must be a whole number" >&2; exit 64; }
[[ -x "$APP" ]] || { echo "no meet-ai program at $APP (pass --app)" >&2; exit 1; }
APP="$(readlink -f "$APP")"
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

OUT="${OUT:-$ROOT/target/tur97-kill-gate/$(date +%Y%m%d-%H%M%S)-linux-kill9-${REC_SECONDS}s}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
SUMMARY="$OUT/summary.txt"
: >"$SUMMARY"

say_line() { echo "$*" | tee -a "$SUMMARY"; }
note() { echo "   $(date +%H:%M:%S) $*" | tee -a "$OUT/run.log"; }

PASSED=0
TOTAL=0
FAILED_ANY=""
# report ok|FAIL|skip <check name> <reason>
report() {
  case "$1" in
    ok) PASSED=$((PASSED + 1)); TOTAL=$((TOTAL + 1)) ;;
    FAIL) TOTAL=$((TOTAL + 1)); FAILED_ANY=1 ;;
  esac
  say_line "$(printf '%-4s  %-34s %s' "$1" "$2" "$3")"
}

# Only processes running the program under test count as "the app": another
# meet-ai build on this machine is never killed.
app_pids() {
  local pid
  for pid in $(pgrep -x meet-ai 2>/dev/null || true); do
    [[ "$(readlink -f "/proc/$pid/exe" 2>/dev/null)" == "$APP" ]] && echo "$pid"
  done
  return 0
}
app_running() { [[ -n "$(app_pids)" ]]; }
app_gone() { ! app_running; }
file_size() { stat -c %s "$1" 2>/dev/null || echo 0; }
now() { date +%s; }
wait_for() {
  local tries=$(($1 * 2)); shift
  while [[ $tries -gt 0 ]]; do
    "$@" && return 0
    sleep 0.5
    tries=$((tries - 1))
  done
  return 1
}
json_field() { python3 -c 'import json,sys; d=json.loads(sys.argv[1]); v='"$2"'; print("" if v is None else v)' "$1"; }
list_meetings() { find "$1" -mindepth 1 -maxdepth 1 -type d -name '*-meeting*' 2>/dev/null | sort; }

TEMP_ROOT="$OUT/meetings-root"
LOG_FILE="$TEMP_ROOT/.app/logs/meet-ai.log"
PLAY_PID=""
MEETING=""

end_app() {
  local pid
  for pid in $(app_pids); do kill -TERM "$pid" 2>/dev/null || true; done
  wait_for "${1:-20}" app_gone && return 0
  for pid in $(app_pids); do kill -9 "$pid" 2>/dev/null || true; done
  wait_for 5 app_gone || true
  return 1
}
cleanup() {
  local rc=$?
  [[ -n "$PLAY_PID" ]] && kill "$PLAY_PID" 2>/dev/null || true
  if app_running; then note "cleanup: meet-ai still running, ending it"; end_app 10 || true; fi
  exit $rc
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# --------------------------------------------------------------------------
# preflight

if [[ -n "$(pgrep -x meet-ai 2>/dev/null || true)" ]]; then
  echo "meet-ai is already running (pid $(pgrep -x meet-ai | tr '\n' ' ')). Quit it first:" >&2
  echo "the gate has to own the only running copy to know which process to kill." >&2
  trap - EXIT
  exit 1
fi

say_line "TUR-97 kill gate (Linux), $(date '+%Y-%m-%d %H:%M:%S')"
say_line "app: $APP"
say_line "mode: record ${REC_SECONDS}s, end=kill -9, meetings root: $TEMP_ROOT"
say_line ""

echo "==> building drift-check (cargo build -p audio --bin drift-check)"
(cd "$ROOT" && cargo build -q -p audio --bin drift-check) >"$OUT/drift-check-build.log" 2>&1 ||
  note "drift-check did not build; check 6 will fail (see drift-check-build.log)"

# The onboarding flag lives inside the meetings root, so a fresh temp root
# would open the setup screen. Mark setup as done for the temp root only.
mkdir -p "$TEMP_ROOT/.app"
printf '{\n  "completedAt": "%s"\n}\n' "$(date --iso-8601=seconds)" >"$TEMP_ROOT/.app/onboarding.json"
list_meetings "$TEMP_ROOT" >"$OUT/temp-root-before.txt"
export MEET_AI_MEETINGS_ROOT="$TEMP_ROOT"

# --------------------------------------------------------------------------
# record, kill -9

note "launching $APP"
"$APP" >"$OUT/app-stdout.log" 2>"$OUT/app-stderr.log" &
disown
if ! wait_for 15 app_running; then
  report FAIL "1 meeting folder" "meet-ai did not start within 15 s"
  exit 1
fi
PID="$(app_pids | head -1)"
note "meet-ai up (pid $PID); waiting 4 s before the toggle"
sleep 4

note "meet-ai --toggle-recording"
"$APP" --toggle-recording >>"$OUT/run.log" 2>&1 || note "the toggle launch exited non-zero (see run.log)"

new_meeting() {
  MEETING="$(list_meetings "$TEMP_ROOT" | comm -13 "$OUT/temp-root-before.txt" - | tail -1)"
  [[ -n "$MEETING" ]]
}
if ! wait_for 20 new_meeting; then
  report FAIL "1 meeting folder" "no new *-meeting folder within 20 s of --toggle-recording"
  exit 1
fi
T0=$(now)
note "recording started: $MEETING"

if [[ -n "$PLAY" ]]; then
  player=""
  for p in pw-play paplay aplay; do command -v "$p" >/dev/null && { player="$p"; break; }; done
  if [[ -n "$player" ]]; then
    note "playing $PLAY with $player"
    "$player" "$PLAY" >/dev/null 2>&1 &
    PLAY_PID=$!
  else
    note "--play given but none of pw-play, paplay, aplay is installed"
  fi
fi

while [[ $(( $(now) - T0 )) -lt $REC_SECONDS ]]; do
  app_running || { note "meet-ai exited on its own during the recording"; break; }
  sleep 1
done
RECORDED=$(( $(now) - T0 ))
note "kill -9 $PID after ${RECORDED}s"
kill -9 "$PID" 2>/dev/null || true
wait_for 5 app_gone || true
[[ -n "$PLAY_PID" ]] && kill "$PLAY_PID" 2>/dev/null || true
PLAY_PID=""

# What the kill left on disk, before the relaunch can repair it.
for ch in mic system; do
  [[ -f "$MEETING/audio/$ch.wav" ]] &&
    python3 "$CHECK" wav "$MEETING/audio/$ch.wav" --expect-seconds "$RECORDED" >"$OUT/after-end-$ch-wav.json" || true
done
[[ -f "$MEETING/audio/segments.json" ]] && cp "$MEETING/audio/segments.json" "$OUT/after-end-segments.json"
ls -la "$MEETING" "$MEETING/audio" >"$OUT/after-end-ls.txt" 2>&1 || true

# --------------------------------------------------------------------------
# check 8: the app starts again with a clean log

off=$(file_size "$LOG_FILE")
note "relaunching $APP"
t0=$(now)
"$APP" >"$OUT/relaunch-app-stdout.log" 2>"$OUT/relaunch-app-stderr.log" &
disown
if ! wait_for 10 app_running; then
  report FAIL "8 relaunch" "meet-ai did not come up within 10 s"
else
  up=$(( $(now) - t0 ))
  sleep 8
  why="up in ${up}s"
  ok=1
  app_running || { why="$why, exited again within 8 s"; ok=""; }
  q0=$(now)
  if end_app 20; then note "SIGTERM ended the relaunch in $(( $(now) - q0 ))s"
  else note "SIGTERM did not end the relaunch within 20 s; killed"; fi
  scan=$(python3 "$CHECK" logscan "$LOG_FILE" "$off" || true)
  echo "$scan" >"$OUT/relaunch-logscan.json"
  [[ "$(json_field "$scan" 'd["ok"]')" == "True" ]] || ok=""
  why="$why, $(json_field "$scan" 'd["reason"]')"
  if [[ -n "$ok" ]]; then report ok "8 relaunch" "$why"; else report FAIL "8 relaunch" "$why"; fi
fi

# --------------------------------------------------------------------------
# checks 1-7 on the meeting as the relaunch left it

audio="$MEETING/audio"
if [[ -d "$audio" ]]; then report ok "1 meeting folder" "$MEETING"
else report FAIL "1 meeting folder" "no audio/ folder in $MEETING"; fi

hdr_ok=1; dur_ok=1; tail_ok=1; hdr_why=""; dur_why=""; tail_why=""
for ch in mic system; do
  wav="$audio/$ch.wav"
  if [[ ! -f "$wav" ]]; then
    hdr_ok=""; dur_ok=""; tail_ok=""; hdr_why="$hdr_why$ch.wav missing; "
    continue
  fi
  json=$(python3 "$CHECK" wav "$wav" --expect-seconds "$RECORDED" || true)
  echo "$json" >"$OUT/final-$ch-wav.json"
  [[ "$(json_field "$json" 'd.get("header_ok")')" == "True" ]] || hdr_ok=""
  hdr_why="$hdr_why$ch: $(json_field "$json" 'd.get("header_reason") or d.get("reason")'); "
  [[ "$(json_field "$json" 'd.get("duration_ok")')" == "False" ]] && dur_ok=""
  dur_why="$dur_why$ch: $(json_field "$json" 'd.get("duration_reason") or d.get("reason")'); "
  [[ "$(json_field "$json" 'd.get("tail_ok")')" == "True" ]] || tail_ok=""
  tail_why="$tail_why$ch: $(json_field "$json" 'd.get("tail_reason") or d.get("reason")'); "
done
if [[ -n "$hdr_ok" ]]; then report ok "2 WAV headers parse + consistent" "${hdr_why%; }"
else report FAIL "2 WAV headers parse + consistent" "${hdr_why%; }"; fi
if [[ -n "$dur_ok" ]]; then report ok "3 declared duration" "${dur_why%; }"
else report FAIL "3 declared duration" "${dur_why%; }"; fi
if [[ -n "$tail_ok" ]]; then report ok "4 PCM beyond header < 6 s" "${tail_why%; }"
else report FAIL "4 PCM beyond header < 6 s" "${tail_why%; }"; fi

json=$(python3 "$CHECK" segments "$audio/segments.json" || true)
echo "$json" >"$OUT/final-segments-check.json"
if [[ "$(json_field "$json" 'd["ok"]')" == "True" ]]; then report ok "5 segments.json" "$(json_field "$json" 'd["reason"]')"
else report FAIL "5 segments.json" "$(json_field "$json" 'd["reason"]')"; fi

# Same acceptance as gate.sh: exit 0, or exit 2 "no checkpoint anchors" for
# a recording shorter than 10 s.
drc=0
(cd "$ROOT" && cargo run -q -p audio --bin drift-check -- "$audio") \
  >"$OUT/drift-check.out" 2>"$OUT/drift-check.err" || drc=$?
if grep -q "invariant violation" "$OUT/drift-check.err"; then
  report FAIL "6 drift-check" "exit $drc, $(grep -m1 'invariant violation' "$OUT/drift-check.err")"
elif [[ $drc -eq 0 ]]; then
  report ok "6 drift-check" "$(grep -m1 'PASS' "$OUT/drift-check.out" || echo 'exit 0')"
elif [[ $drc -eq 2 && $RECORDED -lt 10 ]] && grep -q "no checkpoint anchors" "$OUT/drift-check.err"; then
  report ok "6 drift-check" "exit 2 accepted: recording shorter than 10 s has no checkpoint yet"
else
  report FAIL "6 drift-check" "exit $drc: $(tail -1 "$OUT/drift-check.err")"
fi

report skip "7 system.wav transcribes" "meet-stt is macOS only"

# --------------------------------------------------------------------------

if [[ -z "$FAILED_ANY" && -z "$KEEP" ]]; then
  rm -rf "$MEETING"
  note "all checks passed; deleted the test recording (use --keep to keep it)"
else
  note "test recording kept at $MEETING"
fi
say_line ""
say_line "$PASSED/$TOTAL passed"
say_line "evidence: $OUT"
[[ -z "$FAILED_ANY" ]]
