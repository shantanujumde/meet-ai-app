#!/usr/bin/env bash
# Generate the fixture WAVs named in SPEC §6.
#
# Run via `just fixtures`. Needs ffmpeg and macOS `say`; both are already
# required by the setup. Output is deterministic enough to be regenerated on any
# machine rather than committed — these are ~5 MB of audio and git is the wrong
# place for them (.gitignore excludes *.wav here).
#
# Why synthetic speech rather than a recorded clip: the reference text is the
# point. `reference.json` is the ground truth the Phase-1 accuracy test measures
# word error rate against, so "it reads accurately" becomes a number instead of
# an impression. It also means no real voice, and no real meeting, is ever
# committed to this repo.
#
# These fixtures are NOT a substitute for the Phase-1 exit gate, which requires
# a real recording from `meet-rec`. TTS is cleaner than any microphone: it has
# no room tone, no codec artefacts and no crosstalk. It proves the plumbing and
# the silence gate; it does not prove real-world accuracy.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# 16 kHz mono s16le is what every engine in SPEC §2.5 takes.
readonly RATE=16000

say_clip() {
  # say_clip <voice> <out.wav> <text>
  local voice="$1" out="$2" text="$3"
  # AIFF is big-endian, hence BEI16 rather than the LEI16 a WAVE file would take.
  say -v "$voice" -o "$work/raw.aiff" --file-format=AIFF --data-format=BEI16@22050 "$text"
  ffmpeg -loglevel error -y -i "$work/raw.aiff" -ac 1 -ar "$RATE" -c:a pcm_s16le "$out"
}

# --- 1. silence-30s.wav ------------------------------------------------------
# The hallucination guard (SPEC §6). Digitally perfect silence.
echo "silence-30s.wav"
ffmpeg -loglevel error -y -f lavfi -i "anullsrc=r=$RATE:cl=mono" -t 30 \
  -c:a pcm_s16le silence-30s.wav

# --- 2. room-tone-30s.wav ----------------------------------------------------
# The harder half of the same guard. Digital silence is easy to gate on; a quiet
# room with an air conditioner is what actually makes whisper emit "Thank you."
# VAD has to reject this too, so it gets its own fixture rather than being
# folded into the one above.
echo "room-tone-30s.wav"
ffmpeg -loglevel error -y -f lavfi -i "anoisesrc=r=$RATE:c=pink:a=0.004" -t 30 \
  -ac 1 -c:a pcm_s16le room-tone-30s.wav

# --- 3. two-speaker-60s/ -----------------------------------------------------
# A synthetic two-track meeting: mic.wav is "You", system.wav is "Others", laid
# out the way meet-rec will write them (SPEC §3.1) so the STT side can be tested
# end to end before Phase 0 lands.
echo "two-speaker-60s/"
mkdir -p two-speaker-60s
ref=two-speaker-60s/reference.json

# start_sec|voice|text — start times are where the clip is placed in the track,
# which is also what the transcript timestamp should round down to.
mic_lines=(
  "7|Tom|Sessions are still in memory, that is the blocker."
  "20|Tom|About two days, mostly moving them into Redis."
  "38|Tom|I will write it up after this call."
  "52|Tom|No, that is everything from me."
)
sys_lines=(
  "1|Karen|Morning everyone, let us start with the API work."
  "14|Karen|How long do you think that will take?"
  "30|Karen|Okay, let us put that in a ticket for this sprint."
  "46|Karen|Anything else blocking you this week?"
)

build_track() {
  # build_track <out.wav> <line...>
  local out="$1"
  shift
  local -a lines=("$@")
  local -a inputs=() filters=() mixed=()
  local i=0

  for line in "${lines[@]}"; do
    local start="${line%%|*}" rest="${line#*|}"
    local voice="${rest%%|*}" text="${rest#*|}"
    say_clip "$voice" "$work/clip$i.wav" "$text"
    inputs+=(-i "$work/clip$i.wav")
    filters+=("[$i:a]adelay=$((start * 1000))|$((start * 1000))[d$i]")
    mixed+=("[d$i]")
    i=$((i + 1))
  done

  # normalize=0 keeps each clip at its recorded level; amix's default would
  # divide every sample by the input count and turn speech into near-silence.
  local graph
  graph="$(
    IFS=';'
    echo "${filters[*]}"
  );$(
    IFS=''
    echo "${mixed[*]}"
  )amix=inputs=$i:normalize=0[out]"

  # `-t 60` only truncates; `apad` is what pads the tail out to a full 60 s so
  # both tracks are exactly the same length as segments.json claims.
  ffmpeg -loglevel error -y "${inputs[@]}" -filter_complex "$graph;[out]apad[padded]" \
    -map "[padded]" -t 60 -ac 1 -ar "$RATE" -c:a pcm_s16le "$out"
}

build_track two-speaker-60s/mic.wav "${mic_lines[@]}"
build_track two-speaker-60s/system.wav "${sys_lines[@]}"

# segments.json — the clock-truth record meet-rec writes (SPEC §3.4). One
# segment, both tracks at 16 kHz, 60 s. The STT side reads this to turn a frame
# offset into a transcript timestamp, so the fixture has to carry one.
frames=$((RATE * 60))
cat >two-speaker-60s/segments.json <<JSON
{"segments":[{"idx":0,"start_host_ns":0,"mic_rate":$RATE,"sys_rate":$RATE,
              "mic_frames":$frames,"sys_frames":$frames,"reason":"start"}]}
JSON

# reference.json — ground truth for the word-error-rate assertion.
{
  echo '{"utterances":['
  first=1
  emit_ref() {
    local speaker="$1"
    shift
    for line in "$@"; do
      # One `local` per dependent variable. Bash expands every word of a single
      # `local a=$x b=${a}` before assigning any of them, so a combined
      # declaration would read the PREVIOUS iteration's value of `rest`.
      local start="${line%%|*}"
      local rest="${line#*|}"
      local text="${rest#*|}"
      [ $first -eq 1 ] || echo ','
      first=0
      printf '  {"start_sec":%s,"speaker":"%s","text":%s}' \
        "$start" "$speaker" "$(printf '%s' "$text" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')"
    done
  }
  emit_ref you "${mic_lines[@]}"
  emit_ref others "${sys_lines[@]}"
  echo
  echo ']}'
} >"$ref"

# --- 4. segments/ ------------------------------------------------------------
# The Phase 0 drift-gate fixtures: hand-computed `segments.json` files for the
# §2 drift rows, the §3 refusals and the §4 device switches. Unlike the WAVs
# above these ARE committed — they are a few tens of KB each, and the on-disk
# shape is half of what `crates/audio/tests/segments_fixtures.rs` tests.
#
# Set FIXTURE_WAVS=1 to also write the matching silence WAVs (~1 GB; only a
# future `drift-check` header cross-check needs them).
python3 generate-segments.py

echo "done -> $here"
