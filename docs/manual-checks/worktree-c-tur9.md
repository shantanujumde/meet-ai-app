# Manual checks: c-tur9

TUR-9: the agent setup step (onboarding's last step, after the meetings
folder) and the same card in Settings, under "Notes". Rust side:
`src-tauri/src/agent_setup/` with four commands, `agent_choice`,
`detect_agents`, `save_agent_choice` and `test_agent`. React side:
`src/ui/agent/` and `src/routes/onboarding/AgentStep.tsx`.

The Test button runs the 3-line sample from TUR-4's real-CLI test (Ana: ship
the beta Friday / Ben: release notes by Thursday / Ana: legal sign-off?)
through the real notes run: the wrap-up prompt (the user's template if saved),
the notes schema, no tools, a fresh temp folder, `agent.timeout_sec`. Claude
Code goes through `ClaudeHarness`, Codex through `CodexHarness` (TUR-5, #46,
merged while this ticket was in progress).

## Run by hand

All of these need the running, signed app (`just bundle-signed`, opened from
Finder so it gets Finder's short `PATH`). Agent runs here may not open the app
or run a signed-in CLI, so none was done.

1. **Detection, every state.** Settings → Notes.
   Expect: Claude Code "Signed in" with its version and path
   (`/opt/homebrew/bin/claude`), Codex "Signed in" at
   `/Applications/ChatGPT.app/Contents/Resources/codex`. Then sign Codex out
   (`codex logout`) and press Check again. Expect: "Installed, not signed in"
   and the command `/Applications/ChatGPT.app/Contents/Resources/codex login`
   (full path, since that copy is not on `PATH`). Sign back in.
2. **Test with Claude Code.** Pick Claude Code, model `opus` (the default),
   press Test. Expect: within about a minute, a summary, one task (Ben, release
   notes, Thursday) and one open question (legal sign-off), and the seconds
   it took. Try model `haiku` too; it should be faster.
3. **Test with Codex.** Pick Codex, leave the model blank (Codex's own
   default), press Test. Expect the same kind of result. Then pick a model
   from the suggestion buttons (from `codex debug models`) and test again.
4. **Saved to `config.jsonc`.** After each pick, `cat ~/Meetings/.app/config.jsonc`.
   Expect `agent.harness`, `agent.model` and `agent.binary_path` to match the
   screen, with comments, `auto_run`, `timeout_sec` and other sections kept.
5. **Path picker.** Pick Claude Code, "Choose file…", pick a copy of `claude`
   somewhere else (or a file you then delete, then press Check again). Expect:
   the row shows that path, or "Not found" for the deleted one (a set path is
   the only place looked) and Test is disabled. "Find automatically" puts it
   back.
6. **Bad harness in the file.** Set `"harness": "codx"` in `config.jsonc`, open
   Settings. Expect: an error naming `codx` and "Picking an agent below fixes
   this."; pick one; expect the error gone and the file fixed.
7. **None.** Pick "None, I'll copy the prompt". Expect `agent.harness: "none"`,
   the "Nothing is sent anywhere" line, Test disabled, and the meeting view's
   Copy prompt shown.
8. **Onboarding.** Settings → Show setup again. Expect five dots; the folder
   step says Continue; the agent step is last, and Done finishes setup.
9. **No window, no prompts.** While 1–3 run, watch `ps -ax | grep -E 'claude|codex'`.
   Expect only short-lived processes, no browser window or sign-in prompt.

## Not probed

No real CLI was run from this ticket (0 of the 3 allowed probes used). The
flags were measured by TUR-4 and TUR-5; the new code only feeds them.
`claude auth login` as the sign-in command is assumed, not checked: TUR-6
measured `claude auth status`, but nobody ran `claude auth --help`. If it is
wrong, change `AgentCliId::sign_in_args` in `src-tauri/src/agent_setup/view.rs`.

## Not done here, on purpose

- **The meeting view's Copy prompt rule** (`showsCopyPrompt`'s `cliFound` in
  `src/routes/Review.tsx`) is not wired to detection. It is outside this
  ticket (Settings and onboarding only). `detectAgents` in `src/ipc/client.ts`
  is the call to use there: `cliFound` is "the picked CLI's state is not
  missing".
- **Agent-specific wording in `src/ipc/errors.ts`.** The `agent-*` and
  `unknown-harness` errors show the generic headline with Rust's own sentence
  under it. Better wording belongs with whichever ticket owns that file.

## Failing test outside this ticket

`live_transcript::tests::a_guess_that_is_never_settled_or_withdrawn_does_not_stay_on_screen`
(`src-tauri/src/live_transcript/tests.rs:733`) is flaky on this Mac after the
rebase onto TUR-10: 2 passes in 5 runs of
`cargo test -p meet-ai --lib a_guess_that_is_never_settled`. It is a timing
test (fixed sleeps around the stale-guess timer) in a file this ticket does not
touch, so it was left alone.
