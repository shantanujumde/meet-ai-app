# Manual checks: generated TypeScript bindings (Wave 2, Phase 2)

Everything below needs the real running app, so it was not run here.

## Commands still work in the app

- Run: `just dev`, then open the meetings list, a meeting, Settings, a ticket,
  start and stop a recording, change the meetings folder.
- Expected: every screen loads and every action works as before. Errors still
  show their usual messages (the client now reads the generated result object
  and throws the same `UiError`).
- Why skipped: needs a window, a mic and system audio.
- **Partly run 2026-10-01** on a signed bundle. Meetings list, a meeting,
  onboarding, Settings, model download, and start/stop recording all work,
  and the "config asked for whisper but no model" error showed as a normal
  `UiError` banner. Tickets (list, create, broken file) and changing the
  meetings folder (confirmation, move, refusal banner) also work, so this is
  **passed**.

## Decision taken without an answer from Shann

- Type names in `src/ipc/bindings.ts` are module-prefixed
  (`meet_ai_lib_permission_State`). Two Rust modules both have `State` and
  `Status`, and the namespace layout does not compile under `verbatimModuleSyntax`.
  `client.ts` keeps the hand-written names, so callers see no change. Phase 3 can
  rename the Rust types if cleaner names are wanted.
- `u64`/`usize`/`i64` fields are typed `number` in TypeScript with
  `#[specta(type = ...)]` (specta refuses 64-bit ints by default).
