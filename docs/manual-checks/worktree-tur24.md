# Manual checks: TUR-24 (quality rule R2 is now an ERROR)

Decision taken from the ticket default: yes, R2 fails the gate. `R2_LEVEL=error`
in `scripts/quality-rules.sh`; the R2 heading in `docs/quality-rules.md` now
says ERROR.

Checked here: `scripts/quality-rules.sh` over every tracked `src/**` and
`src-tauri/src/**` file reports no R2 finding, so there was nothing to replace.
The only spelled-out event names outside `events.rs` and `bindings.ts` are in
`//` and `/** */` doc comments, which the rule skips. A throwaway file with
`"recording://state"` gave `R2 ERROR` and exit 2. `bash -n` and `shellcheck`
pass on the script.

## 1. CI fails a PR that spells out an event name

1. On a scratch branch, add `"recording://state"` to any non-test `.ts` file
   under `src/`, push it and open a draft PR.
2. Expected: the quality-rules step in `.github/workflows/check.yml` prints
   `R2 ERROR` for that line and fails. Close the PR afterwards.
3. Not run here: it needs GitHub Actions.

## 2. Stale text in `.claude/skills/quality-gate/SKILL.md`

The table still lists `R2 (warn)`, and lines 69-70 say R2 and R3 print
warnings. This ticket may not edit `.claude/**`, so someone with access should
change the row to `R2 (error)` and leave only R3 in that sentence.

## 3. Other wave-A branches

TUR-27 adds detection events. If its branch spells an event name out instead
of using an `events.rs` constant, its gate now fails after it rebases onto
this change. That is the intended behaviour.
