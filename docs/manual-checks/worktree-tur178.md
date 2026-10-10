# Manual checks: TUR-178

A frontend refactor with no intended behaviour change: one `useCopied` hook,
`useIpcValue` / `useSavedSetting` for load-on-mount settings, a
`recordReminded` / `joinReminded` recording-store action for the detection
prompt, `SettingsRow` for the folder and logs rows with one `SETTINGS_FIELD`
class, shared hooks moved to `src/hooks/`, and `promptPopup.ts` on `call`.
The logic is covered by Vitest (jsdom). Nothing below was run here: each needs
the running app.

## Run by hand

1. Settings → Files: the "Meetings folder" and "Logs" rows.
   Expect: they look as before (icon, bold name, grey line, button on the
   right, path in monospace for the folder). Onboarding's last step shows the
   folder row inside its padded card with no extra padding.
   Why skipped: jsdom has no layout; needs the running app.
2. Click Copy folder path in a meeting, Copy on an agent's sign-in command,
   Copy details on an error, and Start Work on a ticket, twice each about a
   second apart.
   Expect: "Copied" stays for 2 s after the second click, not cut short by the
   first click's timer.
   Why skipped: needs the running app and the clipboard.
3. Settings → Tracker, and the Tickets page's add-a-ticket form.
   Expect: fields look as before.
   Why skipped: needs the running app.
4. A calendar reminder with a link: Join and record, Join, Record.
   Expect: as before; a refused start shows the record button's error banner.
   Why skipped: needs the running app and a calendar event.
5. The prompt popup window (a detected call) right after launch.
   Expect: it shows the card on screen; answering still works.
   Why skipped: needs the running app.

## Known

- Load patterns left as they are, because they are not "load once on mount":
  `useTicketSync` and `PromptPopup` (a read merged with an event stream),
  `SearchResults` (debounced per query), `useCliFound` (a conditional chained
  read), `useConfigProblem` (re-asks on save), `useAgentSetup` (load then
  detect, with run counters), `WatchProblemNote` and `HeadphoneBanner` (event
  plus read).
- Feature-local hooks stay beside their feature (`ui/agent/useAgentSetup`,
  `ui/calendar/useCalendarSignIn`, `ui/engine/useSpeech`, `ui/settings/*`).
