#!/usr/bin/env bash
# A sample meetings folder, so the app shell's populated states can be looked
# at without recording a real meeting.
#
# Why this exists: the Phase 2a screens — the meeting list, the review view,
# the notes pane — are only worth reviewing against real content, and "record a
# 45-minute Zoom call first" is not a reasonable prerequisite for checking that
# a transcript row lines up. `crates/audio/fixtures/generate.sh` does the same
# job for the audio modules.
#
# Writes into target/, which is gitignored, and never touches ~/Meetings.
#
#   just ui-fixtures     # build them
#   just dev-ui          # run the app against them
set -euo pipefail

ROOT="${1:-target/ui-fixtures/Meetings}"
mkdir -p "$ROOT/.app"

# --- A finished, wrapped-up meeting --------------------------------------
# Exercises: a transcript that parses cleanly, an agent-written title from
# meeting.md frontmatter, existing notes, and the "wrapped up" badge.
STANDUP="$ROOT/2026-09-01-1430-platform-standup"
mkdir -p "$STANDUP/audio"

cat >"$STANDUP/transcript.md" <<'EOF'
[00:00:04] Others: Morning everyone, let's start with the API work.
[00:00:11] You: Sessions are still in memory, that's the blocker.
[00:00:19] Others: How long to move them to Redis? I would rather not guess on this one — last time we guessed we were out by a week, and the week was the part nobody had budgeted.
[00:00:31] You: Two days if nothing else lands on top of it.
[00:00:36] Others: Take three. See issue [TUR-17]: the shell work is ahead of schedule anyway.
[00:01:02] You: Fine. I'll open a ticket straight after this.
[00:01:18] Others: Anything else blocking?
[00:01:24] You: Not from me.
EOF

cat >"$STANDUP/meeting.md" <<'EOF'
---
id: 2026-09-01-1430-platform-standup
title: Platform Standup
date: 2026-09-01T14:30:00+05:30
duration_sec: 96
attendees: [Shantanu, Priya]
---

## Summary
## Decisions
## Action Items
## Open Questions
EOF

cat >"$STANDUP/notes.md" <<'EOF'
Redis migration is the blocker. Ask about the session TTL before starting.
EOF

# --- A meeting whose transcript was hand-edited ---------------------------
# Exercises the SPEC §7 path: the parser tolerates a line that is not in the
# §3.4 format, and the UI says so rather than silently dropping it.
REVIEW="$ROOT/2026-09-26-0915-design-review"
mkdir -p "$REVIEW/audio"

cat >"$REVIEW/transcript.md" <<'EOF'
[00:00:02] You: Starting the recording now.
## a heading somebody pasted in by hand
[00:00:14] Others: The glass rim reads better against a light backdrop.
[00:00:21] You: Agreed. I'll raise the rim alpha one step in dark mode.
EOF

# --- A meeting that was started and never spoke into ----------------------
# Exercises the "nothing was transcribed" empty state, which is different from
# "there is no transcript file".
EMPTY="$ROOT/2026-09-27-1100-meeting"
mkdir -p "$EMPTY/audio"
: >"$EMPTY/transcript.md"
: >"$EMPTY/notes.md"

echo "meetings fixture written to $ROOT"
find "$ROOT" -type f | sort | sed 's/^/  /'
