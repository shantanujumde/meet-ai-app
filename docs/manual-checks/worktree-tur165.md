# Manual checks: TUR-165

Onboarding fixes: Continue on the permission step when only System Audio
Recording is off, wizard moves that replace the history entry instead of
pushing one, and no "Show in Finder" button on an empty meetings folder. The
logic is covered by Vitest (`src/routes/Onboarding.test.tsx`,
`src/routes/onboarding/PermissionStep.test.tsx`,
`src/routes/onboarding/FolderStep.test.tsx`). Nothing below was run here: each
needs the running, signed app.

## Run by hand

1. Fresh setup on macOS with System Audio Recording off for meet-ai and the
   Microphone on. Reach the permission step.
   Expect: the red "meet-ai is not allowed to record system audio" panel with
   the numbered steps, both Settings buttons and Check again, and a Continue
   button below it that leads to the Speech step.
   Why skipped: needs the running app and a real permission state.
2. Same, with the Microphone off.
   Expect: "meet-ai is not allowed to record audio", no Continue; only Skip
   setup moves on (unchanged behaviour).
   Why skipped: needs the running app and a real permission state.
3. Go through setup to Done, then look at the title bar.
   Expect: the Back arrow is disabled on Meetings; no press lands back on a
   setup step.
   Why skipped: needs the running app.
4. Settings, change the meetings folder to a new empty folder, then open setup
   from "Fix this" or a fresh config and reach the folder step.
   Expect: the row shows the path and "Change…" only, no "Show in Finder".
   With at least one meeting, "Show in Finder" opens the newest meeting's
   folder.
   Why skipped: needs the running app and Finder.
