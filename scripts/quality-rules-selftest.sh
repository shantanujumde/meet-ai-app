#!/usr/bin/env bash
# quality-rules-selftest.sh — checks that scripts/quality-rules.sh still catches
# what it should, on small sample files in a throwaway git repo.
#
# Usage: scripts/quality-rules-selftest.sh
# Exit 0 when every case behaves, 1 otherwise. Needs only bash, git and awk,
# so it runs on any CI runner. Each case writes one sample file, runs the rules
# on it and checks the exit code and, for a failure, the rule id in the output.
#
# Covers R9 (code adapted from another project). Add cases for other rules
# with the same `expect` helper.
# R10 (OS-specific cfg outside a platform module) has its own `expect10`
# below, since `expect` looks for R9 by name.
#
# Must stay compatible with macOS /bin/bash 3.2.

set -u

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/quality-rules-selftest.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT

# A repo of its own, so the rules' diff base is a commit we control and no
# sample file ever touches the real tree.
repo="$work/repo"
mkdir -p "$repo/scripts" "$repo/src"
cp "$here/quality-rules.sh" "$repo/scripts/quality-rules.sh"
cd "$repo" || exit 1
git init -q
git config user.email selftest@example.invalid
git config user.name selftest
git config commit.gpgsign false

cat >THIRD_PARTY_NOTICES.md <<'EOF'
# Third-party notices

## Example Library

- URL: https://github.com/acme/library
- Licence: MIT
- Copyright: Copyright (c) 2024 Acme
- Commit: 0123456789abcdef0123456789abcdef01234567
- Files: `src/x.rs` from `src/y.rs`

## Codeberg Tool

- URL: https://codeberg.org/someone/tool
- Licence: BSD-2-Clause

## To confirm

- URL: https://github.com/unknown/thing
EOF
git add -A
git commit -q -m base

failed=0
ran=0

# expect pass|fail NAME FILE CONTENT
expect() {
  local want=$1 name=$2 file=$3 content=$4 out code
  ran=$((ran + 1))
  mkdir -p "$(dirname "$file")"
  printf '%s\n' "$content" >"$file"
  out=$(scripts/quality-rules.sh "$file" 2>&1)
  code=$?
  rm -f "$file"
  if [ "$want" = pass ] && [ "$code" -eq 0 ] && ! printf '%s' "$out" | grep -q ' R9 '; then
    echo "ok   $name"
  elif [ "$want" = fail ] && [ "$code" -eq 2 ] && printf '%s' "$out" | grep -q ' R9 ERROR: '; then
    echo "ok   $name"
  else
    echo "FAIL $name: wanted $want, got exit $code"
    [ -n "$out" ] && printf '%s\n' "$out" | sed 's/^/     /'
    failed=$((failed + 1))
  fi
}

sha=0123456789abcdef0123456789abcdef01234567

# The two cases the ticket asks for.
expect fail "R9: adapted line with no notice entry fails" src/a.rs \
  "// Adapted from github.com/other/project/src/lib.rs @ $sha (MIT)
fn a() {}"
expect pass "R9: adapted line with a notice entry passes" src/a.rs \
  "// Adapted from github.com/acme/library/src/y.rs @ $sha (MIT)
fn a() {}"

# Comment styles and languages.
expect pass "R9: # comment in a shell script" src/a.sh \
  "# Adapted from github.com/acme/library/run.sh @ ${sha:0:7} (MIT)"
expect pass "R9: /* */ comment in CSS" src/a.css \
  "/* Adapted from github.com/acme/library/a.css @ $sha (MIT) */"
expect pass "R9: <!-- --> comment in HTML" src/a.html \
  "<!-- Adapted from github.com/acme/library/a.html @ $sha (MIT) -->"
expect pass "R9: codeberg host, any case" src/a.swift \
  "// Adapted from Codeberg.org/Someone/Tool/Sources/A.swift @ $sha (BSD-2-Clause)"
expect pass "R9: https:// prefix is accepted" src/a.ts \
  "// Adapted from https://github.com/acme/library/a.ts @ $sha (MIT)"
expect pass "R9: dual licence with a copyable choice" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha (MIT OR GPL-3.0-only)"
expect pass "R9: the words outside a comment are ignored" src/a.rs \
  'let s = "Adapted from github.com/other/project/x @ main";'
expect fail "R9: no entry, in a yml file" src/a.yml \
  "# Adapted from github.com/other/project/.github/ci.yml @ $sha (MIT)"

# Matching is by whole repo, and "To confirm" is not a notice.
expect fail "R9: a repo that only prefixes a notice URL fails" src/a.rs \
  "// Adapted from github.com/acme/lib/a.rs @ $sha (MIT)"
expect fail "R9: a repo listed only under To confirm fails" src/a.rs \
  "// Adapted from github.com/unknown/thing/a.rs @ $sha (MIT)"

# Licence and pinning.
expect fail "R9: GPL source fails even with an entry" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha (GPL-3.0-only)"
expect fail "R9: AGPL source fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha (AGPL-3.0-or-later)"
expect fail "R9: LGPL source fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha (LGPL-2.1-only)"
expect fail "R9: missing SPDX id fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha"
expect fail "R9: empty SPDX id fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha ()"
expect fail "R9: a branch name instead of a commit fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ main (MIT)"
expect fail "R9: a line without a repo path fails" src/a.rs \
  "// Adapted from the acme library (MIT)"

# R10: OS-specific cfg outside a platform module (TUR-42).
# expect10 pass|fail NAME FILE CONTENT — like `expect`, for rule R10.
expect10() {
  local want=$1 name=$2 file=$3 content=$4 out code
  ran=$((ran + 1))
  mkdir -p "$(dirname "$file")"
  printf '%s\n' "$content" >"$file"
  out=$(scripts/quality-rules.sh "$file" 2>&1)
  code=$?
  rm -f "$file"
  if [ "$want" = pass ] && [ "$code" -eq 0 ] && ! printf '%s' "$out" | grep -q ' R10 '; then
    echo "ok   $name"
  elif [ "$want" = fail ] && [ "$code" -eq 2 ] && printf '%s' "$out" | grep -q ' R10 ERROR: '; then
    echo "ok   $name"
  else
    echo "FAIL $name: wanted $want, got exit $code"
    [ -n "$out" ] && printf '%s\n' "$out" | sed 's/^/     /'
    failed=$((failed + 1))
  fi
}

# The two cases the ticket asks for.
expect10 fail "R10: a stray cfg(target_os) in a crate fails" crates/x/src/a.rs \
  '#[cfg(target_os = "macos")]
fn a() {}'
expect10 pass "R10: the same cfg in the crate's platform module passes" crates/x/src/platform/macos.rs \
  '#[cfg(target_os = "macos")]
fn a() {}'

# Every OS form fails outside a platform module.
expect10 fail "R10: cfg(not(target_os))" crates/x/src/a.rs \
  '#[cfg(not(target_os = "macos"))]
fn a() {}'
expect10 fail "R10: cfg(unix)" crates/x/src/a.rs '#[cfg(unix)]
fn a() {}'
expect10 fail "R10: cfg(windows) in src-tauri" src-tauri/src/a.rs '#[cfg(windows)]
fn a() {}'
expect10 fail "R10: cfg(target_family)" crates/x/src/a.rs '#[cfg(target_family = "unix")]
fn a() {}'
expect10 fail "R10: cfg!(windows)" crates/x/src/a.rs 'fn a() -> bool { cfg!(windows) }'
expect10 fail "R10: cfg_attr on an OS condition" crates/x/src/a.rs \
  '#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct A;'
expect10 fail "R10: an OS inside all(test, ...)" crates/x/src/a.rs \
  '#[cfg(all(test, target_os = "macos"))]
mod e2e;'
expect10 fail "R10: a condition split over lines" crates/x/src/a.rs '#[cfg(any(
    unix,
    windows
))]
fn a() {}'
expect10 fail "R10: after a UTF-8 line" crates/x/src/a.rs 'const E: &str = "é";
#[cfg(unix)]
fn a() {}'
expect10 fail "R10: a tests.rs inside src is not an integration test" crates/x/src/mcp/tests.rs \
  '#[cfg(unix)]
mod fake_cli {}'

# Allowed paths.
expect10 pass "R10: a macos/ folder" crates/x/src/macos/tap.rs '#[cfg(target_os = "macos")]
fn a() {}'
expect10 pass "R10: a windows/ folder" crates/x/src/platform/windows/wasapi.rs '#[cfg(windows)]
fn a() {}'
expect10 pass "R10: a linux/ folder" crates/x/src/linux/pw.rs '#[cfg(target_os = "linux")]
fn a() {}'
expect10 pass "R10: eventkit.rs" crates/calendar/src/eventkit.rs '#[cfg(target_os = "macos")]
fn a() {}'
expect10 pass "R10: a build script" crates/x/build.rs '#[cfg(unix)]
fn a() {}'
expect10 pass "R10: an integration test gating its whole file" crates/x/tests/claude.rs '#![cfg(unix)]'
expect10 pass "R10: Rust outside crates/ and src-tauri/src/" spikes/x/src/a.rs '#[cfg(unix)]
fn a() {}'

# Not OS cfgs.
expect10 pass "R10: cfg(test), features, debug_assertions" crates/x/src/a.rs '#[cfg(test)]
mod tests {}
#[cfg(feature = "stub-audio")]
fn a() {}
#[cfg(any(test, feature = "test-support"))]
fn b() {}
fn c() -> bool { cfg!(debug_assertions) }'
expect10 pass "R10: cfg_attr applying windows_subsystem" src-tauri/src/main.rs \
  '#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]'
expect10 pass "R10: the desktop-vs-mobile gate (android/ios only)" src-tauri/src/a.rs \
  '#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod tray;'
expect10 pass "R10: comments, strings and char literals" crates/x/src/a.rs '// #[cfg(unix)] in a comment
//! `#[cfg(target_os = "macos")]` in a doc comment
/* #[cfg(windows)] in a block comment */
const S: &str = "cfg(unix)";
const R: &str = r#"cfg!(windows) "quoted""#;
const Q: char = '\''"'\'';
fn a() {}'

# Only added lines count: a cfg already in the base passes until it is touched.
mkdir -p crates/x/src
printf '#[cfg(unix)]\nfn a() {}\n' >crates/x/src/old.rs
git add crates/x/src/old.rs
git commit -q -m "old cfg"
ran=$((ran + 1))
printf 'fn b() {}\n' >>crates/x/src/old.rs
if scripts/quality-rules.sh crates/x/src/old.rs >/dev/null 2>&1; then
  echo "ok   R10: a cfg already in the base passes"
else
  echo "FAIL R10: a cfg already in the base passes"
  failed=$((failed + 1))
fi
ran=$((ran + 1))
printf '#[cfg(windows)]\nfn c() {}\n' >>crates/x/src/old.rs
if [ "$(scripts/quality-rules.sh crates/x/src/old.rs 2>&1 | grep -c ' R10 ERROR: ')" = 1 ]; then
  echo "ok   R10: a cfg added to an old file fails, the old one does not"
else
  echo "FAIL R10: a cfg added to an old file fails, the old one does not"
  failed=$((failed + 1))
fi
git checkout -q -- crates/x/src/old.rs

# No notices file at all.
git rm -q THIRD_PARTY_NOTICES.md
expect fail "R9: no THIRD_PARTY_NOTICES.md fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha (MIT)"

echo "$((ran - failed))/$ran passed"
[ "$failed" -eq 0 ]
