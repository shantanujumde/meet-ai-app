#!/bin/sh
# pre-destructive-git.sh — snapshot the shared checkout before a command that
# can destroy uncommitted work.
#
# Runs as a PreToolUse hook on Bash. The hook payload arrives on stdin as JSON;
# rather than parse it, this greps the raw payload for the handful of commands
# that discard working-tree state. A false positive costs one cheap, deduplicated
# snapshot, so the match is deliberately loose — missing a real `git clean` is
# the only outcome that actually hurts.
#
# Always exits 0. This hook must never block another agent's command.

set -u

# Read the payload (bounded, so a huge command line cannot stall the hook).
payload=$(dd bs=65536 count=1 2>/dev/null)

printf '%s' "$payload" | grep -Eq \
	'git[^"]*(clean|reset|restore|stash)|git[^"]*checkout[^"]*(--|\.)|git[^"]*switch[^"]*(-f|--discard-changes)|rm[[:space:]]+-[a-zA-Z]*r' ||
	exit 0

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
snapshot="$repo_root/.claude/hooks/tree-snapshot.sh"
[ -x "$snapshot" ] || exit 0

"$snapshot" >/dev/null 2>&1
exit 0
