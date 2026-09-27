#!/bin/sh
# tree-snapshot.sh — make uncommitted work in the shared checkout recoverable.
#
# Several agent runs write to this one working tree at the same time (TUR-16).
# Uncommitted work therefore has no owner: any run that cleans, resets or
# re-syncs takes everyone's in-flight edits with it, and git holds no copy.
#
# This script records the whole working tree — tracked changes AND untracked,
# non-ignored files — as a commit under refs/tree-snapshot/, so a clobbered
# file is always one `git checkout` away. It writes through a throwaway index
# (GIT_INDEX_FILE), so it never touches the staging area a run is using and
# never modifies a single file in the working tree. It is safe to run at any
# moment, including mid-edit.
#
# Usage:
#   tree-snapshot.sh            take a snapshot (what the hooks call)
#   tree-snapshot.sh list       list snapshots, newest first
#   tree-snapshot.sh show REF   list the files a snapshot differs from HEAD in
#
# Recover a file:
#   git checkout refs/tree-snapshot/<stamp> -- path/to/file
#
# Exits 0 even when it cannot snapshot. Losing a snapshot must never be the
# thing that fails somebody's run.

set -u

KEEP=300                            # snapshots retained; older ones are pruned
SOURCE_DIRS="crates src src-tauri sidecar design-system"
LABEL="${PAPERCLIP_RUN_ID:-${CLAUDE_SESSION_ID:-local}}"

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
[ -n "$repo_root" ] || exit 0
git -C "$repo_root" rev-parse --verify -q HEAD >/dev/null 2>&1 || exit 0

# Inside .git/ on purpose: `git clean -xdf` wipes ignored paths but never
# touches .git, so the audit trail outlives the resets it exists to explain.
log_dir="$(git -C "$repo_root" rev-parse --absolute-git-dir)/tree-guard"
log_file="$log_dir/snapshots.log"

note() { printf '%s\n' "$*"; }

record() {
	mkdir -p "$log_dir" 2>/dev/null || return 0
	printf '%s\t%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$LABEL" "$*" >>"$log_file"
}

list_snapshots() {
	git -C "$repo_root" for-each-ref \
		--sort=-creatordate \
		--format='%(refname)	%(creatordate:iso-strict)	%(subject)' \
		refs/tree-snapshot
}

case "${1:-snapshot}" in
list)
	list_snapshots
	exit 0
	;;
show)
	ref="${2:-}"
	[ -n "$ref" ] || {
		note "usage: tree-snapshot.sh show <ref>"
		exit 0
	}
	git -C "$repo_root" diff --stat HEAD "$ref"
	exit 0
	;;
snapshot) ;;
*)
	note "usage: tree-snapshot.sh [snapshot|list|show <ref>]"
	exit 0
	;;
esac

# Build the tree through a scratch index so the run's own staging area, and
# every file in the working tree, stay exactly as they are.
index=$(mktemp -u "${TMPDIR:-/tmp}/tree-snapshot-index.XXXXXX") || exit 0
trap 'rm -f "$index"' EXIT INT TERM

GIT_INDEX_FILE="$index" git -C "$repo_root" read-tree HEAD 2>/dev/null || exit 0
# -A picks up deletions and untracked files; .gitignore still applies, so
# target/, node_modules/ and .paperclip/ stay out.
GIT_INDEX_FILE="$index" git -C "$repo_root" add -A 2>/dev/null || exit 0
tree=$(GIT_INDEX_FILE="$index" git -C "$repo_root" write-tree 2>/dev/null) || exit 0
[ -n "$tree" ] || exit 0

head_commit=$(git -C "$repo_root" rev-parse HEAD)
head_tree=$(git -C "$repo_root" rev-parse "HEAD^{tree}")

untracked=$(git -C "$repo_root" ls-files --others --exclude-standard -- $SOURCE_DIRS 2>/dev/null)
dirty=$(git -C "$repo_root" status --porcelain 2>/dev/null | wc -l | tr -d ' ')

if [ "$tree" = "$head_tree" ]; then
	record "clean	-	tree matches HEAD, nothing to snapshot"
	exit 0
fi

# Skip if the newest snapshot already holds this exact tree — repeated hooks on
# an unchanged tree should not pile up refs.
latest=$(list_snapshots | head -1 | cut -f1)
if [ -n "$latest" ]; then
	latest_tree=$(git -C "$repo_root" rev-parse "$latest^{tree}" 2>/dev/null)
	if [ "$latest_tree" = "$tree" ]; then
		record "duplicate	$latest	tree unchanged since last snapshot"
		exit 0
	fi
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
ref="refs/tree-snapshot/$stamp-$LABEL"

message="tree snapshot: $dirty uncommitted path(s)

Working tree of the shared checkout, captured automatically so concurrent
agent runs cannot destroy each other's uncommitted work (TUR-16).
Run: $LABEL
Base: $head_commit

Recover a file with:
  git checkout $ref -- <path>"

commit=$(
	GIT_AUTHOR_NAME="tree-snapshot" GIT_AUTHOR_EMAIL="tree-snapshot@local" \
		GIT_COMMITTER_NAME="tree-snapshot" GIT_COMMITTER_EMAIL="tree-snapshot@local" \
		git -C "$repo_root" commit-tree "$tree" -p "$head_commit" -m "$message" 2>/dev/null
) || exit 0
[ -n "$commit" ] || exit 0

git -C "$repo_root" update-ref "$ref" "$commit" 2>/dev/null || exit 0
record "snapshot	$ref	$dirty uncommitted path(s)"

note "tree-snapshot: saved $dirty uncommitted path(s) to $ref"
note "tree-snapshot: recover with  git checkout $ref -- <path>"

if [ -n "$untracked" ]; then
	record "untracked	$ref	$(printf '%s' "$untracked" | tr '\n' ' ')"
	note ""
	note "tree-snapshot: WARNING — untracked source files in the shared checkout."
	note "These exist only in the working tree and in the snapshot above. Another"
	note "run that cleans or resets will delete them. Commit them if they are yours:"
	printf '%s\n' "$untracked" | sed 's/^/  /'
fi

# Prune oldest refs beyond KEEP.
list_snapshots | cut -f1 | tail -n +$((KEEP + 1)) | while read -r old; do
	[ -n "$old" ] && git -C "$repo_root" update-ref -d "$old" 2>/dev/null
done

exit 0
