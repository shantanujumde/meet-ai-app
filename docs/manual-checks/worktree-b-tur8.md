# Manual checks: b-tur8 (TUR-8, copy-prompt paths)

`crates/prompts`: the `start-work.md` template and its renderer.
`src-tauri/src/copy_prompt/`: the `start_work_prompt`, `wrap_up_prompt` and
`copy_prompt_fallback` commands. `src/ui/CopyPromptButton.tsx`: the button
behind **Start Work** (Tickets screen) and **Copy prompt** (meeting view).

## Run by hand

1. **Start Work copies a usable prompt.** In the running app, open Tickets and
   press **Start Work** on a ticket that came from a meeting with a
   `transcript_ref` and a `repo:` key in its `meeting.md`.
   Expect: the button reads "Copied" for a moment; the clipboard holds the
   prompt with the ticket, the transcript lines around the ref, and
   "Work in the repo at `<repo>`". Pasting it into Claude Code started in that
   repo makes the agent read the code, propose a plan, and set
   `status: in_progress` in the ticket file.
   Why skipped: needs the running app, the real clipboard and a signed-in
   agent.
2. **Copy prompt appears only with no agent.** Set `"agent": { "harness":
   "none" }` in `~/Meetings/.app/config.jsonc` and open a finished meeting.
   Expect: a **Copy prompt** button under "Show in Finder" with the hint line.
   Set the harness back to `claude-code`: the button is gone. It is never shown
   while that meeting is recording.
   Why skipped: needs the running app.
3. **The clipboard wrap-up lands files (SPEC Phase 4 gate, last sentence).**
   With the harness at `none`, press **Copy prompt**, paste into an agent with
   file access. Expect: `meeting.md` gets the four sections and
   `analyzed_by: clipboard`; `tickets/TICK-NNNN.md` files appear, numbered
   from one past the highest existing ticket; the meeting view updates within
   ~2 s through the watcher.
   Why skipped: needs a signed-in agent and the running app.
4. **Clipboard refused.** Hard to force on macOS; in a plain browser
   (`pnpm dev` outside Tauri, with clipboard permission denied in the browser's
   site settings) press either button.
   Expect: no error screen; "Couldn't copy to the clipboard…" and a selected,
   read-only text box with the prompt. Covered headless by
   `src/ui/CopyPromptButton.test.tsx`.
   Why skipped: opens a window.

## Known gap

- **"No CLI found" does not show Copy prompt yet.** A11 says the button also
  appears when the chosen agent's CLI is not installed. Per the manager's
  ruling, this ticket does not detect CLIs: `showsCopyPrompt` in
  `src/lib/copyPrompt.ts` takes a `cliFound` flag that defaults to `true`, and
  TUR-6 / TUR-10 (agent CLI detection) pass the real value later.

## Choices made without a ruling

- Excerpt window: 120 s before `transcript_ref` to 60 s after, at most 40
  lines, keeping the lines nearest the ref (`crates/prompts/src/start_work.rs`).
- Ticket lookup for Start Work: the meeting's `tickets/` folder first, then
  the shared `<root>/tickets/` the Tickets screen lists. The next free
  `TICK-NNNN` for the clipboard wrap-up is one past the highest in both.
- Repo for Start Work: the meeting's `repo:` key, then `repos.default` from
  config, else the prompt says to work in the current folder and ask.
- A meeting with an empty `transcript.md` refuses Copy prompt with
  "This meeting has no transcript yet…", rather than copying a prompt with
  nothing to summarize.
- Copy prompt shows on wrapped-up meetings too, so notes can be redone.
