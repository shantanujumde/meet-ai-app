#!/usr/bin/env bash
# shellcheck disable=SC2329  # rule functions are called by name from $RULES
# quality-rules.sh — repo-specific rules that biome, tsc and clippy cannot see.
#
# Usage:
#   scripts/quality-rules.sh <file>...     paths relative to the repo root
#   scripts/quality-rules.sh --print-base  print the commit the rules diff against
#
# Prints one line per finding:
#   path:line RULE LEVEL: message        (LEVEL is ERROR or WARN)
# Exit 0 when there is no ERROR, 2 when there is at least one.
#
# The rules only look at the files you pass. Most of them only look at the
# lines those files ADDED compared to the diff base, so old code does not fail
# the gate; new code does. A file the base does not have counts as all-added.
#
# The diff base is where this branch left main: `git merge-base HEAD
# origin/main` (falling back to `main`, then HEAD). Diffing against HEAD alone
# would miss everything a run already committed; agents usually commit before
# they stop. On main itself (no branch) the base is HEAD, so only uncommitted
# lines count. QUALITY_BASE=<rev> overrides it. This replaces the allow-list
# baseline file the plan first proposed: the base commit IS the baseline.
#
# The reasons behind every rule are in docs/quality-rules.md.
#
# Must stay compatible with macOS /bin/bash 3.2: no associative arrays, no
# mapfile, no ${var,,}.
#
# ---------------------------------------------------------------------------
# How to add a rule
#   1. Write a function `rule_rN` that takes ONE repo-relative path and calls
#      `report LEVEL RULE path line "message"` for each problem. Return early
#      for files the rule does not apply to (check the extension / directory).
#   2. Use the helpers below: `added_lines`, `added_text`, `test_start`,
#      `is_rust_test_file`, `is_ts_test_file`, `is_new_file`.
#   3. If the rule is not ready to fail builds yet, give it a level variable
#      (like R2_LEVEL below) set to warn, and flip it to error later.
#   4. Add `rule_rN` to the RULES list below.
#   5. Document it in docs/quality-rules.md and .claude/skills/quality-gate.
#   A rule that needs to run once (not per file) goes in RUN_ONCE instead.
# ---------------------------------------------------------------------------

set -u

# Severity knobs. "warn" prints but never fails; "error" fails the gate.
# R2 flips to error once src-tauri/src/events.rs exists (Phase 2).
# R3 flips to error once crates/meeting-format exists (Phase 3).
R1_MAX_LINES=${R1_MAX_LINES:-600}
R2_LEVEL=warn
R3_LEVEL=warn
R5_LEVEL=warn

RULES="rule_r1 rule_r2 rule_r3 rule_r4 rule_r5 rule_r6 rule_r8"
# R7 (bindings drift) joins this list once a `just bindings` recipe exists.
RUN_ONCE=""

root=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "quality-rules: not inside a git repository" >&2
  exit 1
}
cd "$root" || exit 1

# The commit the rules diff against (see the header).
diff_base() {
  local b
  if [ -n "${QUALITY_BASE:-}" ] && git rev-parse -q --verify "${QUALITY_BASE}^{commit}" >/dev/null 2>&1; then
    git rev-parse "${QUALITY_BASE}^{commit}"
    return
  fi
  for ref in origin/main main; do
    if b=$(git merge-base HEAD "$ref" 2>/dev/null) && [ -n "$b" ]; then
      echo "$b"
      return
    fi
  done
  git rev-parse HEAD 2>/dev/null
}

BASE=$(diff_base)
if [ "${1:-}" = --print-base ]; then
  echo "$BASE"
  exit 0
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/quality-rules.XXXXXX") || exit 1
trap 'rm -rf "$tmp"' EXIT
findings="$tmp/findings"
: >"$findings"

# --- helpers ---------------------------------------------------------------

# report LEVEL RULE PATH LINE MESSAGE
report() {
  local level
  case $1 in
    error | ERROR) level=ERROR ;;
    *) level=WARN ;;
  esac
  printf '%s:%s %s %s: %s\n' "$3" "$4" "$2" "$level" "$5" >>"$findings"
}

# True when the diff base has no copy of the file (new on this branch).
is_new_file() {
  [ -z "$BASE" ] && return 0
  ! git cat-file -e "$BASE:$1" 2>/dev/null
}

# Print the line numbers (in the current file) that were added compared to the
# diff base, one per line. Every line of a new file counts as added. Cached.
added_lines() {
  local cache
  cache="$tmp/added.$(printf '%s' "$1" | shasum | cut -c1-40)"
  if [ ! -f "$cache" ]; then
    if is_new_file "$1"; then
      awk '{ print NR }' "$1" >"$cache"
    else
      git diff -U0 --no-color --no-ext-diff "$BASE" -- "$1" 2>/dev/null | awk '
        /^@@/ {
          s = $0
          sub(/^@@ -[0-9,]+ \+/, "", s)
          sub(/[ ,].*/, "", s)
          n = s + 0
          inhunk = 1
          next
        }
        !inhunk { next }
        /^\+/ { print n; n++; next }
      ' >"$cache"
    fi
  fi
  cat "$cache"
}

# Line number of the first #[cfg(test)] / #![cfg(test)] in a Rust file, or a
# huge number when there is none. Everything from there on counts as test code.
test_start() {
  awk '/^[[:space:]]*#!?\[cfg\(test\)\]/ { print NR; found = 1; exit }
       END { if (!found) print 999999999 }' "$1"
}

# Rust files that are test code as a whole.
is_rust_test_file() {
  case $1 in
    tests/* | */tests/* | */benches/* | benches/*) return 0 ;;
    *_test.rs | *_tests.rs | */tests.rs | *_e2e.rs) return 0 ;;
  esac
  return 1
}

is_ts_test_file() {
  case $1 in
    *.test.* | *.spec.* | src/test/*) return 0 ;;
  esac
  return 1
}

# Count non-test lines on stdin. $1 = "rs" to stop at the first #[cfg(test)].
count_lines() {
  if [ "$1" = rs ]; then
    awk '/^[[:space:]]*#!?\[cfg\(test\)\]/ { exit } { c++ } END { print c + 0 }'
  else
    awk 'END { print NR + 0 }'
  fi
}

# added_text FILE STOP [OPTOUT]
# Print "lineno<TAB>text" for the added lines of FILE that sit before line STOP
# (999999999 for "whole file"). With OPTOUT, skip lines where OPTOUT appears on
# the same line or the line above.
added_text() {
  added_lines "$1" >"$tmp/nums"
  awk -v stop="$2" -v optout="${3:-}" -v nums="$tmp/nums" '
    BEGIN { while ((getline l < nums) > 0) a[l] = 1 }
    FNR >= stop { exit }
    {
      cur = $0
      skip = optout != "" && (index(cur, optout) || index(prev, optout))
      if ((FNR in a) && !skip) printf "%d\t%s\n", FNR, cur
      prev = cur
    }' "$1"
}

TAB=$(printf '\t')

# --- rules -----------------------------------------------------------------

# R1: a source file is too big (> R1_MAX_LINES non-test lines).
rule_r1() {
  local f=$1 kind now old
  case $f in
    *.rs) is_rust_test_file "$f" && return; kind=rs ;;
    src/ipc/bindings.ts | *.d.ts) return ;;
    *.ts | *.tsx) is_ts_test_file "$f" && return; kind=ts ;;
    *) return ;;
  esac
  now=$(count_lines "$kind" <"$f")
  [ "$now" -gt "$R1_MAX_LINES" ] || return
  if is_new_file "$f"; then
    report error R1 "$f" 1 "new file has $now non-test lines (limit $R1_MAX_LINES); split it into smaller modules"
    return
  fi
  old=$(git show "$BASE:$f" 2>/dev/null | count_lines "$kind")
  if [ "$now" -gt "$old" ]; then
    report error R1 "$f" 1 "grew from $old to $now non-test lines (limit $R1_MAX_LINES); move the new code into its own module"
  else
    report warn R1 "$f" 1 "has $now non-test lines (limit $R1_MAX_LINES); already over before this change, split it when you next work here"
  fi
}

# R2: Tauri event names ("recording://state") live in one place per side.
rule_r2() {
  local f=$1 stop=999999999
  case $f in
    src-tauri/src/events.rs | src/ipc/bindings.ts) return ;;
    *.rs) is_rust_test_file "$f" && return; stop=$(test_start "$f") ;;
    *.ts | *.tsx) is_ts_test_file "$f" && return ;;
    *) return ;;
  esac
  awk -v stop="$stop" 'FNR >= stop { exit }
    /^[[:space:]]*\/\// { next }
    /"[a-z-]+:\/\/[a-z-]+"/ {
      line = $0
      # Real URLs are not event names.
      gsub(/"(https?|wss?|file|asset|tauri|ipc):\/\/[^"]*"/, "\"\"", line)
      if (!match(line, /"[a-z-]+:\/\/[a-z-]+"/)) next
      printf "%d\t%s\n", FNR, substr(line, RSTART, RLENGTH)
    }' "$f" | while IFS="$TAB" read -r n lit; do
    report "$R2_LEVEL" R2 "$f" "$n" "event name $lit is spelled out here; use the constant from src-tauri/src/events.rs (Rust) or src/ipc/bindings.ts (TS)"
  done
}

# R3: the meeting folder layout (file names) is owned by crates/meeting-format.
rule_r3() {
  local f=$1 stop
  case $f in
    crates/meeting-format/*) return ;;
    *.rs) is_rust_test_file "$f" && return ;;
    *) return ;;
  esac
  stop=$(test_start "$f")
  awk -v stop="$stop" 'FNR >= stop { exit }
    /^[[:space:]]*\/\// { next }
    match($0, /"(transcript\.md|notes\.md|segments\.json|meeting\.md|\.app)"/) {
      printf "%d\t%s\n", FNR, substr($0, RSTART, RLENGTH)
    }' "$f" | while IFS="$TAB" read -r n lit; do
    report "$R3_LEVEL" R3 "$f" "$n" "meeting layout name $lit is spelled out here; use the constant from crates/meeting-format"
  done
}

# R4: no new .unwrap() / .expect( in non-test Rust library/app code.
rule_r4() {
  local f=$1 stop
  case $f in
    crates/*/src/bin/* | src-tauri/src/bin/*) return ;;
    crates/*/src/*.rs | src-tauri/src/*.rs) ;;
    *) return ;;
  esac
  is_rust_test_file "$f" && return
  stop=$(test_start "$f")
  added_text "$f" "$stop" "quality: allow-unwrap" | awk -F "$TAB" '{
      code = $2
      # Ignore string literals and // comments: "call .unwrap() here" is text.
      gsub(/"([^"\\]|\\.)*"/, "\"\"", code)
      sub(/\/\/.*/, "", code)
      if (code ~ /\.unwrap\(\)/) print $1 "\t.unwrap()"
      else if (code ~ /\.expect\(/) print $1 "\t.expect("
    }' | while IFS="$TAB" read -r n what; do
    report error R4 "$f" "$n" "new $what in non-test code; return an error with ? instead (or add '// quality: allow-unwrap <reason>' if it truly cannot fail)"
  done
}

# R5: a sync #[tauri::command] that touches disk or the store runs on the main
# thread and freezes the window. Make it async (or #[tauri::command(async)]).
# A heuristic (it cannot see through helper functions), so it only warns.
rule_r5() {
  local f=$1 stop
  case $f in
    src-tauri/src/*.rs) ;;
    *) return ;;
  esac
  is_rust_test_file "$f" && return
  stop=$(test_start "$f")
  added_lines "$f" >"$tmp/nums"
  awk -v stop="$stop" -v nums="$tmp/nums" '
    BEGIN { while ((getline l < nums) > 0) a[l] = 1 }
    FNR >= stop { exit }
    /^[[:space:]]*#\[tauri::command/ {
      attr = FNR; isasync = ($0 ~ /async/); pending = 1; inbody = 0
      changed = (FNR in a); next
    }
    pending && /(^|[[:space:]])fn[[:space:]]/ {
      if ($0 ~ /async[[:space:]]+fn/) isasync = 1
      if (FNR in a) changed = 1
      pending = 0; inbody = 1; hit = 0; name = $0
      sub(/.*fn[[:space:]]+/, "", name); sub(/[^A-Za-z0-9_].*/, "", name)
    }
    inbody {
      code = $0
      sub(/\/\/.*/, "", code)
      if (code ~ /std::fs::/ || code ~ /(meetings|store)::[a-z_]+\(/ ||
          code ~ /(^|[^A-Za-z0-9_:])fs::[a-z_]+\(/) hit = 1
      if ($0 ~ /^}/) {
        if (!isasync && hit) printf "%d\t%s\t%d\n", attr, name, changed
        inbody = 0
      }
    }' "$f" | while IFS="$TAB" read -r n name changed; do
    if [ "$changed" = 1 ]; then
      report "$R5_LEVEL" R5 "$f" "$n" "new sync command '$name' seems to do disk/store work on the main thread; make it 'pub async fn' (and use spawn_blocking for heavy work)"
    else
      report "$R5_LEVEL" R5 "$f" "$n" "sync command '$name' seems to do disk/store work on the main thread; make it async when you next touch it"
    fi
  done
}

# R6: no new inline style={{ }} in React components; use Tailwind classes.
rule_r6() {
  local f=$1
  case $f in
    src/*.tsx) is_ts_test_file "$f" && return ;;
    *) return ;;
  esac
  added_text "$f" 999999999 "quality: allow-style" | while IFS="$TAB" read -r n text; do
    case $text in
      *'style={{'*) report error R6 "$f" "$n" "new inline style={{...}}; use Tailwind utility classes (className) instead (or add {/* quality: allow-style <reason> */})" ;;
    esac
  done
}

# R7: src/ipc/bindings.ts must match what `just bindings` generates.
# Disabled until a `bindings` recipe exists, so the gate never rewrites a file.
# When it lands: regenerate into a temp copy (not in place), compare with
# src/ipc/bindings.ts, report error R7 on a difference, and add rule_r7 to
# RUN_ONCE.
# rule_r7() { ...; }

# R8: src/app.css is being emptied into Tailwind; no new CSS rules in it.
rule_r8() {
  local f=$1
  [ "$f" = src/app.css ] || return
  added_text "$f" 999999999 "quality: allow-css" | while IFS="$TAB" read -r n text; do
    # At-rules (@media, @keyframes, @theme, ...) are wrappers, not rules; the
    # selector lines inside them are still caught.
    case $text in
      *[![:space:]]*) ;;
      *) continue ;;
    esac
    case $(printf '%s' "$text" | sed 's/^[[:space:]]*//') in
      @*) ;;
      *'{'*) report error R8 "$f" "$n" "new CSS rule in app.css; styling moves to Tailwind utilities in the component (or add /* quality: allow-css <reason> */)" ;;
    esac
  done
}

# --- main ------------------------------------------------------------------

files=()
for f in "$@"; do
  f=${f#./}
  [ -f "$f" ] && files+=("$f")
done
[ "${#files[@]}" -gt 0 ] || exit 0

for f in "${files[@]}"; do
  for rule in $RULES; do
    "$rule" "$f"
  done
done
for rule in $RUN_ONCE; do
  "$rule" "${files[@]}"
done

[ -s "$findings" ] || exit 0
# Errors first, then warnings, each sorted by path and line.
{
  grep ' ERROR: ' "$findings" | sort -u -t: -k1,1 -k2,2n
  grep ' WARN: ' "$findings" | sort -u -t: -k1,1 -k2,2n
}
if grep -q ' ERROR: ' "$findings"; then
  exit 2
fi
exit 0
