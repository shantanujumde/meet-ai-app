#!/usr/bin/env bash
# quality-gate.sh — check only the files a run changed: lint, types, tests and
# the repo rules in scripts/quality-rules.sh.
#
# Usage:
#   scripts/quality-gate.sh <file>...                 check these files
#   scripts/quality-gate.sh --from-transcript <jsonl> check the files a Claude
#                                                     session edited
#   scripts/quality-gate.sh                           check git's changed and
#                                                     untracked files
#   scripts/quality-gate.sh --list [...]              print the file list, run
#                                                     nothing
#
# Several agent runs share one working tree (see .claude/hooks/tree-snapshot.sh),
# so a plain `git diff` would also pick up files other runs are editing. The
# transcript mode reads the session's Edit / Write / MultiEdit / NotebookEdit
# tool calls instead, so a run is only held to account for its own files.
#
# Exit codes: 0 pass, 2 fail (short report on stderr), 1 bad usage.
#
# Env knobs:
#   QUALITY_GATE_DISABLE=1     skip everything, exit 0
#   QUALITY_GATE_SKIP_TESTS=1  run lint/format/types/rules, skip vitest and
#                              cargo test
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

# --- collect the file list -------------------------------------------------

raw="$tmp/raw"
: >"$raw"
list_only=0
have_source=0

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

while [ $# -gt 0 ]; do
  case $1 in
    --from-transcript)
      [ $# -ge 2 ] || { echo "quality-gate: --from-transcript needs a path" >&2; exit 1; }
      from_transcript "$2" >>"$raw"
      have_source=1
      shift 2
      ;;
    --list) list_only=1; shift ;;
    -h | --help) sed -n '2,26p' "$0"; exit 0 ;;
    --) shift; for a in "$@"; do echo "$a" >>"$raw"; done; have_source=1; break ;;
    -*) echo "quality-gate: unknown option $1" >&2; exit 1 ;;
    *) echo "$1" >>"$raw"; have_source=1; shift ;;
  esac
done

if [ "$have_source" = 0 ]; then
  { git diff --name-only HEAD 2>/dev/null; git ls-files --others --exclude-standard 2>/dev/null; } >>"$raw"
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

# --- run the checks in parallel --------------------------------------------

names=()
pids=()

# run NAME CMD... — start CMD in the background, output to $tmp/NAME.log
run() {
  local name=$1
  shift
  ("$@") >"$tmp/$name.log" 2>&1 &
  names+=("$name")
  pids+=("$!")
}

if [ "${#web[@]}" -gt 0 ] || [ "${#ts[@]}" -gt 0 ]; then
  if [ ! -d node_modules ]; then
    if ! pnpm install --frozen-lockfile >"$tmp/pnpm-install.log" 2>&1; then
      warn "node_modules is missing and 'pnpm install' failed; JS checks will fail"
    fi
  fi
fi

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
  needs_sidecar=0
  for p in $rs_pkgs; do
    pkg_args+=(-p "$p")
    case $p in stt | meet-ai) needs_sidecar=1 ;; esac
  done

  # crates/stt's tests drive target/meet-stt, and tauri-build refuses to build
  # meet-ai without target/meet-stt-<triple> (both made by `just sidecar`).
  sidecar_ok=1
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

  lint_pkgs=()
  test_args=()
  for p in $rs_pkgs; do
    # Without the sidecar meet-ai does not build at all; stt builds but its
    # tests fail.
    [ "$sidecar_ok" = 0 ] && [ "$p" = meet-ai ] && continue
    lint_pkgs+=(-p "$p")
    [ "$sidecar_ok" = 0 ] && [ "$p" = stt ] && continue
    test_args+=(-p "$p")
  done

  run cargo-fmt cargo fmt --check "${pkg_args[@]}"
  if [ "${#lint_pkgs[@]}" -gt 0 ]; then
    run cargo-clippy cargo clippy "${lint_pkgs[@]}" --all-targets -- -D warnings
  fi
  if [ "$skip_tests" != 1 ] && [ "${#test_args[@]}" -gt 0 ]; then
    run cargo-test cargo test "${test_args[@]}"
  fi
fi

run rules "$root/scripts/quality-rules.sh" "${all[@]}"

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

{
  echo "Quality gate FAILED: ${#failed[@]} check(s) failed on ${#all[@]} changed file(s): ${failed[*]}"
  for name in "${failed[@]}"; do
    echo
    echo "--- $name ---"
    tail -n 40 "$tmp/$name.log"
  done
  echo
  echo "Fix these, then finish again."
} >&2
exit 2
