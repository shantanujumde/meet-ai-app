# Manual checks: b-tur6

TUR-6: `crates/agent/src/detect.rs` finds `claude` and `codex` and asks each
whether it is signed in. Public entry points: `agent::detect::claude(binary_path)`
and `agent::detect::codex(binary_path)`, both `Option<&Path> -> Option<Install>`.
`binary_path` is `agent.binary_path` from the config; the caller passes it in,
so `crates/agent` does not depend on `src-tauri`.

`agent.binary_path` is one key for whichever harness the user picked, so the
caller passes it only to that harness's detect and `None` to the other.

Search order, first hit wins: `binary_path` (if set, nothing else is tried),
`$SHELL -lc 'command -v <name>'`, `~/.local/bin`, `/opt/homebrew/bin`,
`/usr/local/bin`, `~/.claude/local` (Claude only), then app bundles:
`{/Applications,~/Applications}/{ChatGPT,Codex}.app/Contents/Resources/codex`,
and for Claude the desktop app's copy at
`~/Library/Application Support/Claude/claude-code/<newest version>/claude.app/Contents/MacOS/claude`.

Sign-in: `claude auth --help` (is there a line starting with `status`?), then
`claude auth status` (JSON, reads only `loggedIn`). If `auth --help` fails,
the answer is signed out. If it works but lists no `status`, an old Claude
Code gets a tiny notes run through `ClaudeHarness` itself (same no-tools,
no-MCP, no-hooks flags; model `haiku`; 60 s limit). Codex: `codex login status`,
signed in when a line starts with "Logged in" (Codex prints it on stderr).

`ClaudeHarness::detect` now calls `detect::claude(binary)`.

## Probed on this Mac (2026-10-02, 2 of 3 allowed probes)

- `claude auth status` (Claude Code 2.1.286, Homebrew cask): exit 0, JSON with
  `"loggedIn": true`, `authMethod`, account email and org. So the `-p`
  fallback is not needed on current versions.
- `/Applications/ChatGPT.app/Contents/Resources/codex login status`: exit 0,
  `Logged in using ChatGPT` on **stderr**, stdout empty.
- `/bin/zsh -lc 'command -v claude; command -v codex'` from this session:
  `/opt/homebrew/bin/claude`; no `codex` (it is only inside ChatGPT.app).
- `codex --version` and `claude auth --help` were not probed; the fake
  scripts assume `--version` prints one line and that `auth --help` mentions
  `status`.

## Run by hand

1. Signed release bundle: `just bundle-signed`, open the `.app` from Finder
   (not `tauri dev`, which inherits Terminal's `PATH`), open the setup screen.
   Expect: Claude Code found at `/opt/homebrew/bin/claude`, version
   `2.1.286 (Claude Code)` or newer, signed in; Codex found at
   `/Applications/ChatGPT.app/Contents/Resources/codex`, signed in.
   Why skipped: needs a signed bundle and the running app, and the setup
   screens and the Codex call site are not on main yet (see below).
   **TUR-6 is not done until this check passes** (ticket "Done when").
2. Same, without opening the app: `cargo test -p agent -- --ignored real_`.
   Expect: `real_clis_are_found` passes.
   Why skipped: runs the real, signed-in CLIs; agent runs here may not run
   `--ignored` tests.
3. Set `agent.binary_path` to a wrong path. Expect: setup shows the CLI as
   not found (no fallback to another copy). Set it back to null. Expect: found
   again.
   Why skipped: needs the running app.
4. While check 1 runs, watch `ps -ax | grep -E 'claude|codex|mcp'`.
   Expect: no MCP server process and no browser window; only short-lived
   `--version` / `auth status` / `login status` processes.
   Why skipped: needs the running app and the real CLIs.

## Not done here, on purpose

- **Codex call site.** `codex.rs` (TUR-5, #46) was not on main at this
  rebase. Agreed with Shann: TUR-5 puts `detect::codex(binary_path)` in its
  `Harness::detect` stub when it rebases.
- **`PATH` for the runs themselves.** Detection runs the CLI with its own
  folder first on `PATH` (`process::cli_command`), so an npm install
  (`#!/usr/bin/env node`, with `node` next to it) works from Finder. The notes
  and sync runners (TUR-4, TUR-5) need the same; they can use
  `process::cli_command` / `process::search_path_with`.
- **Shell rc files that print a path-looking line** after `command -v` could
  be picked instead; only an existing executable is accepted, so the risk is
  low.
