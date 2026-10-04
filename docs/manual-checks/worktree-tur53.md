# Manual checks: TUR-53 (find claude / codex on Windows and Linux)

Checked headless on the Mac:

- `cargo test -p agent`: `detect::find_tests` (the shared search order on a
  fake home laid out like this OS: every install folder with every program
  ending, earlier folders first, `PATH` via `which` before folders,
  `binary_path` winning and taking a missing `.exe`/`.cmd`, a non-program file
  skipped) and `platform::macos::tests` (the macOS folder list is unchanged).
  The existing unix `detect::tests` pass unchanged apart from `shell` being an
  `Option`.
- `cargo test -p meet-ai agent_setup`: the macOS sign-in commands are the same
  as before; new tests for the Linux (bare name) and PowerShell (bare name when
  the folder is on `PATH`, else `& '<path>'` with `'` doubled) styles run on
  every OS.
- `cargo clippy` for `agent` (macOS and `x86_64-pc-windows-msvc`) and
  `meet-ai`, `pnpm vitest run src/ui/agent src/ui/NotesRun`, `pnpm typecheck`.

The `rust (windows)` and `rust (linux)` CI jobs run `cargo test --workspace`,
so `detect::find_tests`, `platform::windows::tests` and
`platform::linux::tests` run there. See the PR for their results.

## 1. Settings shows a real Claude Code as found and signed in, on Windows

Not run: no Windows machine, and it needs a real signed-in CLI.

1. On Windows, install Claude Code with the native installer
   (`irm https://claude.ai/install.ps1 | iex`) and sign in
   (`claude auth login` in PowerShell). Separately, try an npm install
   (`npm i -g @anthropic-ai/claude-code`, which leaves `claude.cmd` in
   `%APPDATA%\npm`).
2. Start meet-ai and open Settings, Agent.
3. Expected: the Claude Code card says found and signed in, with the path
   `C:\Users\<you>\.local\bin\claude.exe` (native) or
   `...\AppData\Roaming\npm\claude.cmd` (npm). "Test" succeeds, which shows a
   `.cmd` shim starts.
4. Sign out (`claude auth logout`), press check again: signed out, and the card
   says "Run this in PowerShell" with `claude auth login` (or `& '<full path>'
   auth login` when that folder is not on `PATH`). Pasting it in PowerShell
   works.
5. Same for Codex (`npm i -g @openai/codex`, `codex login`).

## 2. The same on Linux

Not run: no Linux desktop here.

1. Install Claude Code into `~/.local/bin` (native installer) or with npm into
   `~/.npm-global/bin`; start meet-ai from the desktop launcher (not a
   terminal, so its `PATH` is the session's).
2. Expected: found and signed in; when signed out the card shows
   `claude auth login` after "Run this in Terminal".

## Notes

- `src/lib/osText.ts` (TUR-51) is not on main yet, so the "PowerShell" /
  "Terminal" word comes from `src/ui/agent/terminalName.ts`; move it there once
  TUR-51 merges.
- The macOS-only lookups (Claude desktop's versioned copy, `.app` bundles) stay
  in the shared `Cli` table; on Windows and Linux there are no app folders to
  search and the versioned folder never exists, so they find nothing.
