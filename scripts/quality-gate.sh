#!/usr/bin/env bash
# quality-gate.sh — check only the files a run changed: lint, types, tests and
# the repo rules in scripts/quality-rules.sh.
#
# Usage:
#   scripts/quality-gate.sh <file>...                 check exactly these files
#   scripts/quality-gate.sh --from-transcript <jsonl> check the files a Claude
#                                                     session changed
#   scripts/quality-gate.sh                           check everything this
#                                                     branch changed
#   scripts/quality-gate.sh --list [...]              print the file list, run
#                                                     nothing
#
# Which files: a run is held to account for the files IT changed, not the
# whole repo. The transcript mode reads the session's Edit / Write / MultiEdit
# / NotebookEdit tool calls. A session with none of those and no Bash call is
# read-only and passes at once. Edits made through the Bash tool (sed -i, a
# heredoc) are not in the list, so when the run sits in its own linked
# worktree on its own branch (the normal case, see CONTRIBUTING.md "Agent runs
# and the working tree") and made at least one edit or Bash call, the gate
# adds every file the branch changed since it left main. The primary checkout
# only ever uses the transcript list, because other work may be sitting there.
#
# Exit codes: 0 pass (or timed out, see QUALITY_GATE_BUDGET_SECS), 2 fail
# (short report on stderr), 1 bad usage.
#
# Env knobs:
#   QUALITY_GATE_DISABLE=1          skip everything, exit 0
#   QUALITY_GATE_SKIP_TESTS=1       run lint/format/types/rules, skip vitest and
#                                   cargo test
#   QUALITY_GATE_BUDGET_SECS=480    time limit for all checks together; past it
#                                   the checks are killed and the gate does not
#                                   block (a cold worktree build can be slow)
#   QUALITY_GATE_TRANSCRIPT_ONLY=1  do not add the branch's changed files in
#                                   transcript mode
#   QUALITY_GATE_INCONCLUSIVE_EXIT  exit code for "timed out" (default 0; the
#                                   hook sets 3 so it knows not to cache a pass)
#   QUALITY_BASE=<rev>              diff base for the rules (default: where the
#                                   branch left main)
#
# Compatible with macOS /bin/bash 3.2 (no associative arrays, no mapfile).

set -u

[ "${QUALITY_GATE_DISABLE:-0}" = 1 ] && exit 0

root=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "quality-gate: not inside a git repository" >&2
  exit 1
}
root=$(cd "$root" && pwd -P) || exit 1
cd "$root" || exit 1

tmp=$(mktemp -d "${TMPDIR:-/tmp}/quality-gate.XXXXXX") || exit 1
trap 'rm -rf "$tmp"' EXIT

budget=${QUALITY_GATE_BUDGET_SECS:-480}
case $budget in '' | *[!0-9]*) budget=480 ;; esac
inconclusive_exit=${QUALITY_GATE_INCONCLUSIVE_EXIT:-0}

# One diff base for the gate and the rules.
QUALITY_BASE=$("$root/scripts/quality-rules.sh" --print-base 2>/dev/null)
export QUALITY_BASE

# --- collect the file list -------------------------------------------------

raw="$tmp/raw"
: >"$raw"
list_only=0
explicit=0
transcript=0
calls=0

# Print the file_path of every file-editing tool call in a transcript JSONL.
from_transcript() {
  local t=$1
  if [ ! -f "$t" ]; then
    echo "quality-gate: transcript not found: $t" >&2
    return
  fi
  if command -v jq >/dev/null 2>&1; then
    jq -R -r 'fromjson? | select(type == "object")
      | .message.content? | arrays | .[]
      | select(type == "object" and .type == "tool_use")
      | select(.name == "Edit" or .name == "Write" or .name == "MultiEdit" or .name == "NotebookEdit")
      | (.input.file_path // .input.notebook_path // empty)' "$t" 2>/dev/null
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "$t" <<'PY'
import json, sys
TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
with open(sys.argv[1], encoding="utf-8", errors="replace") as fh:
    for line in fh:
        try:
            content = json.loads(line).get("message", {}).get("content")
        except (ValueError, AttributeError):
            continue
        if not isinstance(content, list):
            continue
        for block in content:
            if isinstance(block, dict) and block.get("type") == "tool_use" and block.get("name") in TOOLS:
                inp = block.get("input") or {}
                path = inp.get("file_path") or inp.get("notebook_path")
                if path:
                    print(path)
PY
  else
    # Last resort: rough, but only ever over-reports.
    grep -E '"name":"(Edit|Write|MultiEdit|NotebookEdit)"' "$t" |
      grep -oE '"(file_path|notebook_path)":"[^"]+"' |
      sed -E 's/^"[a-z_]+":"//; s/"$//'
  fi
}

# Files this branch changed since the diff base, plus untracked files.
branch_files() {
  [ -n "$QUALITY_BASE" ] && git diff --name-only "$QUALITY_BASE" -- 2>/dev/null
  git ls-files --others --exclude-standard 2>/dev/null
}

# Count the tool calls in a transcript that can change files: Edit, Write,
# MultiEdit, NotebookEdit and Bash. Zero means a read-only session.
change_calls() {
  local t=$1
  [ -f "$t" ] || { echo 0; return; }
  if command -v jq >/dev/null 2>&1; then
    jq -R -r 'fromjson? | select(type == "object")
      | .message.content? | arrays | .[]
      | select(type == "object" and .type == "tool_use")
      | select(.name == "Edit" or .name == "Write" or .name == "MultiEdit"
               or .name == "NotebookEdit" or .name == "Bash")
      | .name' "$t" 2>/dev/null | wc -l | tr -d ' '
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "$t" <<'PY'
import json, sys
TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"}
n = 0
with open(sys.argv[1], encoding="utf-8", errors="replace") as fh:
    for line in fh:
        try:
            content = json.loads(line).get("message", {}).get("content")
        except (ValueError, AttributeError):
            continue
        if isinstance(content, list):
            n += sum(1 for b in content if isinstance(b, dict)
                     and b.get("type") == "tool_use" and b.get("name") in TOOLS)
print(n)
PY
  else
    grep -cE '"name":"(Edit|Write|MultiEdit|NotebookEdit|Bash)"' "$t"
  fi
}

# True when HEAD is a branch other than main/master.
on_own_branch() {
  local b
  b=$(git symbolic-ref --short -q HEAD 2>/dev/null) || return 1
  case $b in
    '' | main | master) return 1 ;;
  esac
  return 0
}

# True when this checkout is a linked worktree (`git worktree add`), not the
# primary checkout. Only then does the branch belong to one run; the primary
# checkout can hold other runs' files.
in_linked_worktree() {
  local gd cd_
  gd=$(cd "$(git rev-parse --git-dir 2>/dev/null)" 2>/dev/null && pwd -P) || return 1
  cd_=$(cd "$(git rev-parse --git-common-dir 2>/dev/null)" 2>/dev/null && pwd -P) || return 1
  [ "$gd" != "$cd_" ]
}

while [ $# -gt 0 ]; do
  case $1 in
    --from-transcript)
      [ $# -ge 2 ] || { echo "quality-gate: --from-transcript needs a path" >&2; exit 1; }
      from_transcript "$2" >>"$raw"
      transcript=1
      calls=$((calls + $(change_calls "$2")))
      shift 2
      ;;
    --list) list_only=1; shift ;;
    -h | --help) sed -n '2,41p' "$0"; exit 0 ;;
    --) shift; for a in "$@"; do echo "$a" >>"$raw"; done; explicit=1; break ;;
    -*) echo "quality-gate: unknown option $1" >&2; exit 1 ;;
    *) echo "$1" >>"$raw"; explicit=1; shift ;;
  esac
done

if [ "$explicit" = 0 ] && [ "$transcript" = 0 ]; then
  branch_files >>"$raw"
elif [ "$transcript" = 1 ]; then
  # A session that never edited a file or ran a shell command changed
  # nothing: do not hold it to account for what the branch already had.
  if [ "$calls" = 0 ] && [ "$explicit" = 0 ]; then
    exit 0
  fi
  # Bash-tool edits are not in the transcript list. In a run's own linked
  # worktree the branch is that run's work, so add what the branch changed.
  # Never in the primary checkout, which other runs share.
  if [ "$calls" -gt 0 ] && [ "${QUALITY_GATE_TRANSCRIPT_ONLY:-0}" != 1 ] &&
    in_linked_worktree && on_own_branch; then
    branch_files >>"$raw"
  fi
fi

# Keep files that still exist, sit inside this repo and are not git-ignored.
# Prints repo-relative paths, one per line, sorted and unique.
normalize() {
  local p dir rel
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    case $p in
      /*) ;;
      *) p="$root/$p" ;;
    esac
    [ -f "$p" ] || continue
    dir=$(cd "$(dirname "$p")" 2>/dev/null && pwd -P) || continue
    p="$dir/$(basename "$p")"
    case $p in
      "$root"/*) rel=${p#"$root"/} ;;
      *) continue ;;
    esac
    git check-ignore -q -- "$rel" 2>/dev/null && continue
    printf '%s\n' "$rel"
  done
}

normalize <"$raw" | sort -u >"$tmp/files"

if [ "$list_only" = 1 ]; then
  cat "$tmp/files"
  exit 0
fi
[ -s "$tmp/files" ] || exit 0

# --- sort files into groups ------------------------------------------------

web=()     # biome
ts=()      # typecheck + vitest related
rs_pkgs="" # space-separated cargo package names
all=()

# Cargo package name for a crate directory, from its Cargo.toml.
package_name() {
  sed -n '/^\[package\]/,/^\[/ s/^name *= *"\([^"]*\)".*/\1/p' "$1/Cargo.toml" 2>/dev/null | head -n 1
}

add_pkg() {
  [ -n "$1" ] || return
  case " $rs_pkgs " in
    *" $1 "*) ;;
    *) rs_pkgs="$rs_pkgs $1" ;;
  esac
}

while IFS= read -r f; do
  all+=("$f")
  case $f in
    *.ts | *.tsx | *.mts | *.cts)
      web+=("$f")
      ts+=("$f")
      ;;
    *.js | *.jsx | *.mjs | *.cjs | *.json | *.jsonc | *.css) web+=("$f") ;;
  esac
  case $f in
    crates/*/*.rs | crates/*/Cargo.toml)
      d=${f#crates/}
      d="crates/${d%%/*}"
      add_pkg "$(package_name "$d")"
      ;;
    src-tauri/*.rs | src-tauri/Cargo.toml)
      add_pkg "$(package_name src-tauri)"
      ;;
  esac
done <"$tmp/files"
rs_pkgs=${rs_pkgs# }

skip_tests=${QUALITY_GATE_SKIP_TESTS:-0}
warnings="$tmp/warnings"
: >"$warnings"
warn() { echo "quality-gate WARN: $*" >>"$warnings"; }

# --- setup that the checks depend on ---------------------------------------

if [ "${#web[@]}" -gt 0 ] && [ ! -d node_modules ]; then
  if ! pnpm install --frozen-lockfile >"$tmp/pnpm-install.log" 2>&1; then
    warn "node_modules is missing and 'pnpm install' failed; JS checks will fail"
  fi
fi

sidecar_ok=1
if [ -n "$rs_pkgs" ]; then
  needs_sidecar=0
  for p in $rs_pkgs; do
    case $p in stt | meet-ai) needs_sidecar=1 ;; esac
  done
  # crates/stt's tests drive target/meet-stt, and tauri-build refuses to build
  # meet-ai without target/meet-stt-<triple> (both made by `just sidecar`).
  target_dir=${CARGO_TARGET_DIR:-target}
  if [ "$needs_sidecar" = 1 ] && [ ! -x "$target_dir/meet-stt" ]; then
    if command -v just >/dev/null 2>&1 && command -v swiftc >/dev/null 2>&1 &&
      just sidecar >"$tmp/sidecar.log" 2>&1; then
      :
    else
      sidecar_ok=0
      warn "target/meet-stt is missing and 'just sidecar' could not build it; skipping cargo checks that need it (stt tests, meet-ai)"
    fi
  fi
fi

# --- run the checks in parallel --------------------------------------------

# Job control: every background job gets its own process group, so a timeout
# can kill a job together with everything it started (cargo, rustc, vitest
# workers) and leave no orphan holding the cargo target lock.
set -m

names=()
pids=()

# run NAME CMD... — start CMD in the background, output to $tmp/NAME.log
run() {
  local name=$1
  shift
  "$@" </dev/null >"$tmp/$name.log" 2>&1 &
  names+=("$name")
  pids+=("$!")
}

if [ "${#web[@]}" -gt 0 ]; then
  run biome pnpm exec biome check --no-errors-on-unmatched --files-ignore-unknown=true "${web[@]}"
fi
if [ "${#ts[@]}" -gt 0 ]; then
  run typecheck pnpm typecheck
  if [ "$skip_tests" != 1 ]; then
    run vitest pnpm exec vitest related --run --passWithNoTests "${ts[@]}"
  fi
fi

if [ -n "$rs_pkgs" ]; then
  pkg_args=()
  lint_pkgs=()
  test_args=()
  for p in $rs_pkgs; do
    pkg_args+=(-p "$p")
    # Without the sidecar meet-ai does not build at all; stt builds but its
    # tests fail.
    [ "$sidecar_ok" = 0 ] && [ "$p" = meet-ai ] && continue
    lint_pkgs+=(-p "$p")
    [ "$sidecar_ok" = 0 ] && [ "$p" = stt ] && continue
    test_args+=(-p "$p")
  done

  run cargo-fmt cargo fmt --check "${pkg_args[@]}"
  # clippy and test share the target lock, so they queue behind each other
  # anyway; the time budget below bounds the total.
  if [ "${#lint_pkgs[@]}" -gt 0 ]; then
    run cargo-clippy cargo clippy "${lint_pkgs[@]}" --all-targets -- -D warnings
  fi
  if [ "$skip_tests" != 1 ] && [ "${#test_args[@]}" -gt 0 ]; then
    run cargo-test cargo test "${test_args[@]}"
  fi
fi

run rules "$root/scripts/quality-rules.sh" "${all[@]}"

# Wait for every job, or until the budget runs out.
timed_out=0
while [ -n "$(jobs -rp)" ]; do
  if [ "$SECONDS" -ge "$budget" ]; then
    timed_out=1
    break
  fi
  sleep 1
done

if [ "$timed_out" = 1 ]; then
  still=""
  i=0
  # stderr is muted here so bash's "Terminated" job notices stay out of the
  # report.
  {
    while [ "$i" -lt "${#pids[@]}" ]; do
      if kill -0 "${pids[$i]}" 2>/dev/null; then
        still="$still ${names[$i]}"
        kill -TERM -- "-${pids[$i]}"
      fi
      i=$((i + 1))
    done
    sleep 2
    for pid in "${pids[@]}"; do
      kill -KILL -- "-$pid"
    done
    wait
  } 2>/dev/null
  cat "$warnings" >&2
  echo "quality-gate WARN: gate timed out after ${budget}s (still running:$still); not blocking. Run 'just check' (or scripts/quality-gate.sh) by hand." >&2
  exit "$inconclusive_exit"
fi

failed=()
i=0
while [ "$i" -lt "${#pids[@]}" ]; do
  if ! wait "${pids[$i]}"; then
    failed+=("${names[$i]}")
  fi
  i=$((i + 1))
done

# --- report ----------------------------------------------------------------

cat "$warnings" >&2

if [ "${#failed[@]}" -eq 0 ]; then
  exit 0
fi

# The useful part of a cargo log is the error and panic lines, which a plain
# tail often cuts off. Show those first.
excerpt() {
  local log="$tmp/$1.log" key
  case $1 in
    cargo-*)
      key=$(grep -E '^(error|warning)(\[|:)|panicked at|^test .* FAILED|^failures:|^---- ' "$log" | head -n 30)
      if [ -n "$key" ]; then
        printf '%s\n' "$key"
        echo "... last lines:"
      fi
      if [ "$1" = cargo-test ]; then tail -n 80 "$log"; else tail -n 40 "$log"; fi
      ;;
    *) tail -n 40 "$log" ;;
  esac
}

{
  echo "Quality gate FAILED: ${#failed[@]} check(s) failed on ${#all[@]} changed file(s): ${failed[*]}"
  for name in "${failed[@]}"; do
    echo
    echo "--- $name ---"
    excerpt "$name"
  done
  echo
  echo "Fix these, then finish again."
} >&2
exit 2
