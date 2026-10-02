# Manual checks: b-tur4

TUR-4: the Claude Code runner, `crates/agent/src/claude.rs` (SPEC A11). It
builds the notes and sync commands, pipes the prompt in on stdin, and reads
`structured_output` from the CLI's JSON envelope. `process.rs` gains
`run_cli_exit`, which hands back the exit code and stdout of a failed run, so
a "Not logged in" envelope becomes `AgentError::NotSignedIn` instead of a
bare CLI failure. Stdout is only read to classify the failure; it never goes
into an error, the UI or a log.

Tested headless with a fake `claude` script on `PATH`
(`crates/agent/tests/claude.rs`): the exact arguments for both run kinds, the
prompt on stdin, the JSON envelope, a non-zero exit, bad JSON, a missing
`structured_output`, a not-signed-in envelope and a missing binary.

## Probed once by hand (2026-10-02, Claude Code 2.1.286)

The notes command, exactly as `ClaudeHarness::args` builds it, on the 3-line
sample with `--model haiku`, from an empty folder in `/tmp`: exit 0, about
5 s, $0.012. The envelope had `is_error: false`, `subtype: "success"` and a
`structured_output` with the two tasks (Ben: release notes, Ana: ship the
beta) and the legal sign-off question. No nested-session error, even though it
was started from inside a Claude Code session.

## Run by hand

1. `cargo test -p agent --test claude -- --ignored real_cli_notes_run_on_three_line_transcript`
   Expect: passes in about 10 s, at least one task and one open question.
   Why skipped: `--ignored` tests are not allowed in agent runs; it needs the
   signed-in `claude` CLI and spends a few cents. The same command was
   probed once by hand above and worked.
2. `cargo test -p agent --test claude -- --ignored real_cli_sync_run_refuses_bash`
   Expect: passes; the file the prompt asks Bash to `touch` does not exist
   afterwards, and the envelope's `permission_denials` lists the Bash call.
   Why skipped: same as 1. Not probed, to keep within the 3-probe limit.
3. Sign out of Claude Code (`claude` then `/logout`), then run test 1 again.
   Expect: it fails with "Claude Code is installed but not signed in". This
   checks the real CLI's wording still matches `SIGN_IN_HINTS` in
   `claude.rs` ("Not logged in", "/login", "Invalid API key", ...).
   Why skipped: signing out changes the user's machine and needs a login
   afterwards.
4. Open the app from Finder (signed bundle), run the Settings "Test" button.
   Expect: notes come back. If it says "Claude Code is not installed", the
   app did not pass the login shell's `PATH` to `with_search_path`; the CLI is
   a node script and needs `node` on `PATH` too.
   Why skipped: needs the signed bundle, the running app and the setup screen
   (TUR-6 wires detection, later tickets wire the app).

## Notes

- `detect()` is a stub that returns `None` (agreed with Shann): TUR-6 ships
  `detect::claude(binary_path)`, and whichever ticket merges last adds the
  call.
- A sync job with no tools is refused before the CLI starts. A notes job
  ignores any tools set on it and always runs with `--tools ""`.
- A model or tool name that is empty or starts with `-` is refused, so it can
  never be read as a flag. Tool names with a comma are refused too, since
  `--allowedTools` gets them comma-separated.
