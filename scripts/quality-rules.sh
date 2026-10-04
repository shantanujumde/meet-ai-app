#!/usr/bin/env bash
# shellcheck disable=SC2329  # rule functions are called by name from $RULES
# quality-rules.sh — repo-specific rules that biome, tsc and clippy cannot see.
#
# Usage:
#   scripts/quality-rules.sh <file>...     paths relative to the repo root
#   scripts/quality-rules.sh --print-base  print the commit the rules diff against
#   scripts/quality-rules.sh --r10-tree    R10 only, over every tracked .rs file and
#                                          every line (CI; see R10 below)
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
#      (like R3_LEVEL below) set to warn, and flip it to error later.
#   4. Add `rule_rN` to the RULES list below.
#   5. Document it in docs/quality-rules.md and .claude/skills/quality-gate.
#   A rule that needs to run once (not per file) goes in RUN_ONCE instead.
# ---------------------------------------------------------------------------

set -u

# Severity knobs. "warn" prints but never fails; "error" fails the gate.
# R3 flips to error once crates/meeting-format exists (Phase 3).
R1_MAX_LINES=${R1_MAX_LINES:-600}
R2_LEVEL=error
R3_LEVEL=warn
R5_LEVEL=warn

RULES="rule_r1 rule_r2 rule_r3 rule_r4 rule_r5 rule_r6 rule_r8"
RULES="$RULES rule_r9"
RUN_ONCE="rule_r7"
# R10 (TUR-42): OS-specific cfg outside a platform module.
RULES="$RULES rule_r10"
# R11 (TUR-92): em dash in user-facing text.
RULES="$RULES rule_r11"

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
# --r10-tree (TUR-89): R10 on the whole tree, not only on added lines, so two
# branches that each pass on their own cannot add up to a cfg in shared code.
R10_TREE=0
if [ "${1:-}" = --r10-tree ]; then
  R10_TREE=1
  shift
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

# Where the test code of a Rust file starts. That is a `#[cfg(test)]` whose
# next real line (skipping blanks, comments and more attributes) is a `mod`
# item: the usual `mod tests { ... }` at the bottom. A `#[cfg(test)]` on a
# single fn or impl block mid-file does NOT start the test region, or the prod
# code after it would go unchecked. `#![cfg(test)]` makes the whole file test.
# mode=start prints that line number (999999999 when there is none);
# mode=count prints how many lines come before it (all lines when none).
# shellcheck disable=SC2016  # an awk program: $0 is awk's, not the shell's
TEST_START_AWK='
  pending {
    if ($0 ~ /^[[:space:]]*$/ || $0 ~ /^[[:space:]]*#\[/ || $0 ~ /^[[:space:]]*\/\//) next
    if ($0 ~ /^[[:space:]]*(pub(\([a-z]+\))?[[:space:]]+)?mod[[:space:]]/) { found = start; exit }
    pending = 0
  }
  /^[[:space:]]*#!\[cfg\(test\)\]/ { found = 1; exit }
  /^[[:space:]]*#\[cfg\(test\)\]/ { pending = 1; start = NR; next }
  END {
    if (mode == "count") print (found ? found - 1 : NR + 0)
    else print (found ? found : 999999999)
  }'

test_start() {
  awk -v mode=start "$TEST_START_AWK" "$1"
}

# Print the line numbers that belong to a single #[cfg(test)] item (a
# test-only fn or impl block mid-file), by counting braces from the attribute
# until the item closes. Strings and // comments are stripped first.
cfg_test_item_lines() {
  awk '
    !initem && /^[[:space:]]*#\[cfg\(test\)\]/ { initem = 1; depth = 0; opened = 0 }
    initem {
      print NR
      code = $0
      gsub(/"([^"\\]|\\.)*"/, "\"\"", code)
      sub(/\/\/.*/, "", code)
      if (code ~ /^[[:space:]]*#\[/) next
      opens = gsub(/\{/, "{", code)
      closes = gsub(/\}/, "}", code)
      depth += opens - closes
      if (opens > 0) opened = 1
      if ((opened && depth <= 0) || (!opened && code ~ /;[[:space:]]*$/)) initem = 0
    }' "$1"
}

# Rust files that are test code as a whole.
is_rust_test_file() {
  case $1 in
    tests/* | */tests/* | */benches/* | benches/*) return 0 ;;
    *_test.rs | *_tests.rs | */tests.rs | *_e2e.rs | */e2e.rs) return 0 ;;
  esac
  return 1
}

is_ts_test_file() {
  case $1 in
    *.test.* | *.spec.* | src/test/*) return 0 ;;
  esac
  return 1
}

# Count non-test lines on stdin. $1 = "rs" to stop where the test module starts
# (see TEST_START_AWK).
count_lines() {
  if [ "$1" = rs ]; then
    awk -v mode=count "$TEST_START_AWK"
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
  cfg_test_item_lines "$f" >"$tmp/testitems"
  added_text "$f" "$stop" "quality: allow-unwrap" | awk -F "$TAB" -v skip="$tmp/testitems" '
    BEGIN { while ((getline l < skip) > 0) t[l] = 1 }
    ($1 in t) { next }
    {
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
# Runs once, and only when a changed file can change the output (command files,
# the types they send, the generator itself, or bindings.ts). It generates into
# a temp copy through BINDINGS_OUT, so the gate never rewrites a working-tree
# file. Takes the whole changed-file list.
rule_r7() {
  local f touched=0
  for f in "$@"; do
    case $f in
      src/ipc/bindings.ts | src-tauri/src/*.rs | src-tauri/Cargo.toml | Cargo.toml | crates/stt/src/*.rs | crates/meeting-format/src/*.rs) touched=1 ;;
    esac
  done
  [ "$touched" = 1 ] || return
  [ -f src/ipc/bindings.ts ] || return
  command -v cargo >/dev/null || return
  # tauri-build wants the sidecar next to the source (target/meet-stt-<host
  # triple>, from `just sidecar`), whatever CARGO_TARGET_DIR says. A worktree
  # sharing a build cache often has it only there. Without it meet-ai does not
  # build, so this is a skipped check (a WARN, as the gate does for stt), not
  # a stale bindings file (TUR-89).
  local host sidecar_msg
  host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
  sidecar_msg="skipped: target/meet-stt-${host:-<host>} is missing, and meet-ai does not build without it; run \`just sidecar\` (and copy target/meet-stt* into \$CARGO_TARGET_DIR if you set it), then the gate again"
  if [ -n "$host" ] && [ ! -e "target/meet-stt-$host" ]; then
    report warn R7 src/ipc/bindings.ts 1 "$sidecar_msg"
    return
  fi
  local out="$tmp/bindings.ts"
  if ! BINDINGS_OUT="$out" cargo test -p meet-ai --lib export_bindings >"$tmp/bindings.log" 2>&1; then
    # Only tauri-build's own complaint, so a real compile error in a file
    # that happens to mention the sidecar is not hidden.
    if grep -Eq "resource path .*meet-stt[^ ]* doesn't exist" "$tmp/bindings.log"; then
      report warn R7 src/ipc/bindings.ts 1 "$sidecar_msg"
      return
    fi
    report error R7 src/ipc/bindings.ts 1 "could not generate the bindings (cargo test -p meet-ai --lib export_bindings failed); run \`just bindings\` to see why"
    return
  fi
  if ! cmp -s "$out" src/ipc/bindings.ts; then
    report error R7 src/ipc/bindings.ts 1 "generated bindings are out of date; run \`just bindings\` and commit src/ipc/bindings.ts"
  fi
}

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

# R9: code copied or adapted from another project. A comment
#   Adapted from <repo>/<path> @ <commit> (<SPDX>)
# with <repo> written host/owner/name (github.com/insidegui/AudioCap) must have
# a section in THIRD_PARTY_NOTICES.md whose URL line names that repo, and the
# licence must be one we may copy from. Looks at every line of the file, not
# only the added ones: the notice has to exist for as long as the copy does.
# The rules in full: CONTRIBUTING.md, "Code from other projects".
R9_NOTICES=${R9_NOTICES:-THIRD_PARTY_NOTICES.md}

# True when R9_NOTICES has a section (not "## To confirm") with a URL line that
# names $1 (lowercase host/owner/name) as a whole repo, not a prefix of one.
# With $2 (licence ids, space-separated), that section's Licence line must also
# name one of them: for an `X OR Y` source, the licence we took it under.
r9_has_notice() {
  [ -f "$R9_NOTICES" ] || return 1
  awk -v repo="$1" -v choices="${2:-}" '
    function close_section() {
      if (urlhit && (choices == "" || lichit)) found = 1
      urlhit = 0; lichit = 0
    }
    BEGIN { nc = split(tolower(choices), C, " ") }
    /^##[[:space:]]/ {
      close_section()
      s = tolower($0)
      sub(/^##[[:space:]]+/, "", s)
      sub(/[[:space:]]+$/, "", s)
      ok = (s != "to confirm")
      next
    }
    ok && /^[[:space:]]*(-[[:space:]]+)?Licen[cs]e:/ {
      l = " " tolower($0) " "
      for (k = 1; k <= nc; k++) {
        if ((i = index(l, C[k])) > 0 && substr(l, i - 1, 1) !~ /[a-z0-9.-]/ &&
            substr(l, i + length(C[k]), 1) !~ /[a-z0-9.-]/) lichit = 1
      }
    }
    ok && /^[[:space:]]*(-[[:space:]]+)?URL:/ {
      l = tolower($0)
      off = 0
      while ((i = index(substr(l, off + 1), repo)) > 0) {
        at = off + i
        before = at > 1 ? substr(l, at - 1, 1) : ""
        after = substr(l, at + length(repo), 1)
        if ((before == "" || before ~ /[\/[:space:]<(]/) &&
            (after == "" || after ~ /[\/[:space:]>)#]/ || substr(l, at + length(repo), 4) == ".git")) {
          urlhit = 1
          break
        }
        off = at
      }
    }
    END { close_section(); exit found ? 0 : 1 }' "$R9_NOTICES"
}

# The licences we may copy from: CONTRIBUTING.md, "Code from other projects",
# COPY allowed (TUR-89). An allow-list, so a licence nobody thought of fails.
R9_ALLOWED="MIT Apache-2.0 BSD-2-Clause BSD-3-Clause ISC Zlib Unlicense MPL-2.0"

# Prints why the SPDX expression $1 cannot be copied from (the first licence id
# that sinks it), or nothing when it can. `X OR Y` means we may pick one, so it
# passes when either side does; `X AND Y` binds both, so each must pass
# (AND binds tighter than OR, parentheses group); `X WITH <exception>` is
# judged by X. Ids match without regard to case, as SPDX says.
# shellcheck disable=SC2016  # an awk program: $0 is awk's, not the shell's
r9_licence_problem() {
  awk -v expr="$1" -v allowed="$R9_ALLOWED" '
    function f(   r, id) {
      if (T[p] == "(") {
        p++; r = or_(); if (T[p] == ")") p++; else syntax = 1
        return r
      }
      id = T[p]; p++
      if (tolower(T[p]) == "with") p += 2
      if (id == "" || id == ")" || tolower(id) ~ /^(or|and|with)$/) { syntax = 1; return 0 }
      if (tolower(id) in ok) return 1
      if (bad == "") bad = id
      return 0
    }
    function and_(   r, r2) {
      r = f()
      while (tolower(T[p]) == "and") { p++; r2 = f(); r = r && r2 }
      return r
    }
    function or_(   r, r2) {
      r = and_()
      while (tolower(T[p]) == "or") { p++; r2 = and_(); r = r || r2 }
      return r
    }
    BEGIN {
      n = split(allowed, A, " ")
      for (i = 1; i <= n; i++) ok[tolower(A[i])] = 1
      e = expr; gsub(/\(/, " ( ", e); gsub(/\)/, " ) ", e)
      nt = split(e, T, " ")
      p = 1
      r = or_()
      if (p <= nt) syntax = 1
      if (!r || syntax) print (bad != "" ? bad : expr)
    }'
}

# Prints the ids in the SPDX expression $1 that are on R9_ALLOWED, one per line.
r9_allowed_parts() {
  local word allowed
  for word in $(printf '%s' "$1" | tr '()' '  '); do
    for allowed in $R9_ALLOWED; do
      if [ "$(printf '%s' "$word" | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$allowed" | tr '[:upper:]' '[:lower:]')" ]; then
        echo "$allowed"
      fi
    done
  done
}

rule_r9() {
  local f=$1
  case $f in
    # The rule's own code and tests spell the format out as examples.
    scripts/quality-rules.sh | scripts/quality-rules-selftest.sh) return ;;
    *.rs | *.ts | *.tsx | *.js | *.jsx | *.mjs | *.cjs | *.swift | *.sh | *.yml | *.yaml) ;;
    *.cmake | CMakeLists.txt | */CMakeLists.txt | *.css | *.html | *.c | *.h | *.m | *.mm | *.py | *.toml | justfile) ;;
    *) return ;;
  esac
  local n text repo commit spdx rest problem choices
  # Group 2 is the repo, 3 the upstream path, 4 the commit, 5 what follows.
  local re='Adapted from[[:space:]]+(https?://)?([A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+/[^/[:space:]]+/[^/[:space:]]+)/([^[:space:]]+)[[:space:]]+@[[:space:]]+([^[:space:]]+)(.*)$'
  # The SPDX expression in parentheses, with one level of grouping inside it.
  local spdx_re='^[[:space:]]*[(](([^()]|[(][^()]*[)])*)[)]'
  local format='Adapted from <host>/<owner>/<repo>/<path> @ <commit> (<SPDX>)'
  awk '/(\/\/|#|\/\*|<!--|^[[:space:]]*\*)[[:space:]]*Adapted from/ { printf "%d\t%s\n", FNR, $0 }' "$f" |
    while IFS="$TAB" read -r n text; do
      if ! [[ $text =~ $re ]]; then
        report error R9 "$f" "$n" "'Adapted from' line is not in the form '$format'"
        continue
      fi
      repo=$(printf '%s' "${BASH_REMATCH[2]}" | tr '[:upper:]' '[:lower:]')
      repo=${repo%.git}
      commit=${BASH_REMATCH[5]}
      rest=${BASH_REMATCH[6]}
      if ! [[ $commit =~ ^[0-9a-fA-F]{7,40}$ ]]; then
        report error R9 "$f" "$n" "'Adapted from' must pin a commit hash (7-40 hex digits), not '$commit': licences change, a branch name moves"
        continue
      fi
      spdx=""
      if [[ $rest =~ $spdx_re ]]; then
        spdx=$(printf '%s' "${BASH_REMATCH[1]}" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
      fi
      if [ -z "$spdx" ]; then
        report error R9 "$f" "$n" "'Adapted from' line has no SPDX licence id; write it as '$format'. No licence means we may not copy it"
        continue
      fi
      problem=$(r9_licence_problem "$spdx")
      case $problem in
        '') ;;
        GPL* | AGPL* | LGPL* | gpl* | agpl* | lgpl*)
          report error R9 "$f" "$n" "code adapted from $repo is $spdx; $problem code is inspiration only, never copied (CONTRIBUTING.md, \"Code from other projects\")"
          continue
          ;;
        *)
          report error R9 "$f" "$n" "code adapted from $repo is $spdx; $problem is not one we may copy from (only ${R9_ALLOWED// /, }, 'X OR Y' when at least one side is, and then the notice names the one we took; 'X AND Y' when every part is; CONTRIBUTING.md, \"Code from other projects\")"
          continue
          ;;
      esac
      if ! r9_has_notice "$repo"; then
        report error R9 "$f" "$n" "adapted code without a notice: add a '## <Project>' section with 'URL: https://$repo' to $R9_NOTICES"
        continue
      fi
      # A choice of licences: the notice says which one we took (TUR-89).
      if printf ' %s ' "$spdx" | tr '()' '  ' | grep -qi ' or '; then
        choices=$(r9_allowed_parts "$spdx" | tr '\n' ' ')
        if ! r9_has_notice "$repo" "$choices"; then
          report error R9 "$f" "$n" "$spdx is a choice of licences: the Licence line of $repo's section in $R9_NOTICES must name the one we took it under (one of: ${choices% }), e.g. 'Licence: MIT (chosen from $spdx)'"
        fi
      fi
    done
}

# R10: OS-specific cfg outside a platform module (SPEC §8.2, TUR-42).
#
# Step 1 (R10_CLEAN_AWK) prints the file with comments removed, char literals
# blanked and every string literal (raw strings too) cut down to its letters,
# digits and underscores, so step 2 can match parentheses and words without a
# "cfg(unix)" in a string or a comment counting. Line numbers are kept.
# shellcheck disable=SC2016  # awk programs: $0 is awk's, not the shell's
R10_CLEAN_AWK='
  function flush() { print out; out = "" }
  {
    line = $0; n = length(line); i = 1
    while (i <= n) {
      c = substr(line, i, 1)
      if (mode == "block") {
        if (substr(line, i, 2) == "*/") { mode = ""; i += 2 } else i++
        continue
      }
      if (mode == "str") {
        if (c == "\\") { i += 2; continue }
        if (c == "\"") { out = out "\""; mode = ""; i++; continue }
        if (c ~ /[A-Za-z0-9_]/) out = out c
        i++; continue
      }
      if (mode == "raw") {
        if (substr(line, i, length(rawend)) == rawend) { out = out "\""; mode = ""; i += length(rawend); continue }
        if (c ~ /[A-Za-z0-9_]/) out = out c
        i++; continue
      }
      prev = (i > 1) ? substr(line, i - 1, 1) : " "
      if (substr(line, i, 2) == "//") break
      if (substr(line, i, 2) == "/*") { mode = "block"; i += 2; continue }
      if (c == "r" && prev !~ /[A-Za-z0-9_]/ && match(substr(line, i + 1), /^#*"/)) {
        hashes = RLENGTH - 1
        rawend = "\""; for (h = 0; h < hashes; h++) rawend = rawend "#"
        out = out "\""; mode = "raw"; i += RLENGTH + 1; continue
      }
      if (c == "\"") { out = out "\""; mode = "str"; i++; continue }
      if (c == "'\''") {
        if (substr(line, i + 1, 1) == "\\") {
          j = index(substr(line, i + 2), "'\''")
          if (j) { out = out "'\'' '\''"; i += j + 2; continue }
        } else if (substr(line, i + 2, 1) == "'\''") {
          out = out "'\'' '\''"; i += 3; continue
        }
      }
      out = out c; i++
    }
    flush()
  }'

# Step 2 (R10_FIND_AWK) reads the cleaned text and prints "first last" (line
# numbers) for every cfg(...), cfg!(...) and cfg_attr(...) whose condition
# names an OS: target_os, target_family, target_vendor, unix or windows. For
# cfg_attr only the condition counts, not the attribute it applies, so
# `cfg_attr(not(debug_assertions), windows_subsystem = "windows")` passes.
# A condition whose only OS names are target_os = "android" / "ios" is Tauri's
# desktop-vs-mobile gate, not a port, and passes too.
# shellcheck disable=SC2016
R10_FIND_AWK='
  function judge(kind, text,   cond, depth, k, ch, bare, words, w, v, os, mobile) {
    cond = text
    if (kind == "cfg_attr") {
      depth = 0
      for (k = 1; k <= length(text); k++) {
        ch = substr(text, k, 1)
        if (ch == "(") depth++
        else if (ch == ")") depth--
        else if (ch == "," && depth == 0) { cond = substr(text, 1, k - 1); break }
      }
    }
    os = 0; mobile = 1
    v = cond
    while (match(v, /target_os[[:space:]]*=[[:space:]]*"[^"]*"/)) {
      w = substr(v, RSTART, RLENGTH); sub(/^[^"]*"/, "", w); sub(/"$/, "", w)
      if (w != "android" && w != "ios") mobile = 0
      v = substr(v, RSTART + RLENGTH)
    }
    bare = cond
    gsub(/"[^"]*"/, "", bare)
    n = split(bare, words, /[^A-Za-z0-9_]+/)
    for (k = 1; k <= n; k++) {
      w = words[k]
      if (w == "target_os" || w == "target_family" || w == "target_vendor" || w == "unix" || w == "windows") {
        os = 1
        if (w != "target_os") mobile = 0
      }
    }
    return os && !mobile
  }
  {
    line[NR] = $0
  }
  END {
    for (r = 1; r <= NR; r++) {
      # The leading space stands in for "start of line" (no ^ inside a group:
      # mawk on the ubuntu runners is pickier than macOS awk).
      rest = " " line[r]
      while (match(rest, /[^A-Za-z0-9_]cfg(_attr|!)?[[:space:]]*\(/)) {
        kind = substr(rest, RSTART, RLENGTH)
        kind = (kind ~ /cfg_attr/) ? "cfg_attr" : "cfg"
        # Collect from just after the opening paren to its matching one,
        # across lines if the condition is split.
        depth = 1; text = ""; rr = r
        seg = substr(rest, RSTART + RLENGTH)
        rest = seg
        while (depth > 0 && rr <= NR) {
          for (k = 1; k <= length(seg) && depth > 0; k++) {
            ch = substr(seg, k, 1)
            if (ch == "(") depth++
            else if (ch == ")") { depth--; if (depth == 0) break }
            text = text ch
          }
          if (depth > 0) { rr++; if (rr - r > 40) break; seg = line[rr]; text = text " " }
        }
        if (judge(kind, text)) print r " " rr
      }
    }
  }'

# Known R10 debt, one `path|line` entry per line, where line is the cfg's
# first line with the indent trimmed. Keyed on the text, not the line number,
# so an edit above it does not turn the tree run red. Each entry lets through
# one hit, the first one in the file with that text, so a new cfg in the same
# file still fails (even one spelled the same). Matters for --r10-tree only: on
# added lines, an untouched old cfg never counts. Better than editing an entry:
# make the test portable and drop it.
#   crates/agent/src/process.rs     TUR-54 removes this (unix-only /bin/sh fake harness tests)
#   crates/agent/src/detect.rs      TUR-54 removes this (same)
#   crates/agent/src/mcp/tests.rs   TUR-54 removes this (same)
R10_DEBT='crates/agent/src/process.rs|#[cfg(unix)]
crates/agent/src/detect.rs|#[cfg(unix)]
crates/agent/src/mcp/tests.rs|#[cfg(unix)]'

rule_r10() {
  local f=$1 first last
  case $f in
    crates/*.rs | src-tauri/src/*.rs) ;;
    *) return ;;
  esac
  case $f in
    */platform/* | */macos/* | */windows/* | */linux/* | */eventkit.rs | */build.rs) return ;;
  esac
  # Integration tests may gate a whole OS-only file; tests/ in a crate's src does not count.
  [[ $f =~ ^crates/[^/]+/tests/ ]] && return
  added_lines "$f" >"$tmp/r10nums"
  # Bytes, not characters: macOS awk gives up on some UTF-8 text otherwise.
  # --r10-tree: every line counts (the whole file is "added").
  [ "$R10_TREE" = 1 ] && awk '{ print NR }' "$f" >"$tmp/r10nums"
  # Debt entries used so far in this file, one per line (see R10_DEBT).
  local seen="" key
  LC_ALL=C awk "$R10_CLEAN_AWK" "$f" | LC_ALL=C awk "$R10_FIND_AWK" | while read -r first last; do
    key="$f|$(sed -n "${first}p" "$f" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
    if [ "$(printf '%s\n' "$seen" | grep -cFx -- "$key")" -lt "$(printf '%s\n' "$R10_DEBT" | grep -cFx -- "$key")" ]; then
      seen="$seen$key"$'\n'
      [ "$R10_TREE" = 1 ] && report warn R10 "$f" "$first" "known OS cfg outside a platform module (R10_DEBT in scripts/quality-rules.sh); TUR-54 removes it"
      continue
    fi
    # Only a cfg with at least one added line counts.
    if awk -v a="$first" -v b="$last" '$1 >= a && $1 <= b { found = 1; exit } END { exit !found }' "$tmp/r10nums"; then
      report error R10 "$f" "$first" "OS-specific cfg outside a platform module; put the OS code in the crate's src/platform/ (macos.rs / windows.rs / linux.rs) and call platform::... from here (SPEC §8.2)"
    fi
  done
}

# R11 (TUR-92): no em dash in user-facing text. Whole file, not only added
# lines, so one that slipped in earlier is caught the next time the file
# changes. Comments are skipped: // to end of line (not the // in "https://"),
# and /* ... */ blocks across lines (JSX {/* */} too). Rust: src-tauri/src only,
# code before the test module; strings there reach the window, the tray and
# notifications.
# shellcheck disable=SC2016  # an awk program: $0 is awk's, not the shell's
R11_AWK='
  FNR >= stop { exit }
  {
    line = $0; code = ""
    while (length(line) > 0) {
      if (inblock) {
        e = index(line, "*/")
        if (!e) { line = ""; break }
        line = substr(line, e + 2); inblock = 0; continue
      }
      s = index(line, "/*")
      if (s) { code = code substr(line, 1, s - 1); line = substr(line, s + 2); inblock = 1; continue }
      code = code line; line = ""
    }
    while (match(code, /(^|[^:])\/\//)) code = substr(code, 1, RSTART)
    if (index(code, "\342\200\224")) print FNR
  }'
rule_r11() {
  local f=$1 stop=999999999
  case $f in
    src/ipc/bindings.ts | *.d.ts) return ;;
    src/*.ts | src/*.tsx) is_ts_test_file "$f" && return ;;
    src-tauri/src/*.rs)
      is_rust_test_file "$f" && return
      stop=$(test_start "$f")
      ;;
    *) return ;;
  esac
  LC_ALL=C awk -v stop="$stop" "$R11_AWK" "$f" | while read -r n; do
    report error R11 "$f" "$n" "em dash (—) in user-facing text; use a full stop, comma, colon or brackets instead (docs/quality-rules.md R11)"
  done
}

# --- main ------------------------------------------------------------------

files=()
for f in "$@"; do
  f=${f#./}
  [ -f "$f" ] && files+=("$f")
done
if [ "$R10_TREE" = 1 ]; then
  RULES=rule_r10
  RUN_ONCE=""
  files=()
  while IFS= read -r f; do
    [ -f "$f" ] && files+=("$f")
  done < <(git ls-files '*.rs')
fi
# R9 reads the notices file, so a change to it (a section dropped, or the file
# deleted) can break files nobody touched: then R9 checks every file with an
# `Adapted from` line too (TUR-89).
r9_extra=()
if [ "$R10_TREE" = 0 ]; then
  for f in "$@"; do
    if [ "${f#./}" = "$R9_NOTICES" ]; then
      while IFS= read -r f; do
        [ -f "$f" ] && r9_extra+=("$f")
      done < <(git grep -l --untracked 'Adapted from' 2>/dev/null)
      break
    fi
  done
fi
[ "${#files[@]}" -gt 0 ] || [ "${#r9_extra[@]}" -gt 0 ] || exit 0

if [ "${#files[@]}" -gt 0 ]; then
  for f in "${files[@]}"; do
    for rule in $RULES; do
      "$rule" "$f"
    done
  done
  for rule in $RUN_ONCE; do
    "$rule" "${files[@]}"
  done
fi
if [ "${#r9_extra[@]}" -gt 0 ]; then
  for f in "${r9_extra[@]}"; do
    rule_r9 "$f"
  done
fi

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
