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

# No notices file at all.
git rm -q THIRD_PARTY_NOTICES.md
expect fail "R9: no THIRD_PARTY_NOTICES.md fails" src/a.rs \
  "// Adapted from github.com/acme/library/a.rs @ $sha (MIT)"

echo "$((ran - failed))/$ran passed"
[ "$failed" -eq 0 ]
