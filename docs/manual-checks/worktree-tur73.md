# Manual checks: TUR-73 (agent picker radio buttons too small to see or click)

These need the running app, so they were not run here. The behaviour is
covered headless by `src/ui/Radio.test.tsx` (label click on name and detail,
arrow keys and Tab focus, disabled cannot be picked) and
`src/ui/agent/AgentOption.test.tsx` (row click, arrow keys through the three
agents, the status pill is not part of the radio's name), using
`@testing-library/user-event` for real key presses.

Cause: the old radio was `size-4`, which in this theme is `--space-4` = 8 px
(the 4pt grid, not Tailwind's rem steps), drawn by WebKit's native control at
that size. The new `src/ui/Radio.tsx` is a native `<input type="radio">`
restyled with `appearance: none`, inside a `<label>` that wraps the row.

What was checked here: the built CSS (`vite build` into /tmp) contains every
utility the radio uses (`size-6` = 16 px, `border-[1.5px]`,
`checked:border-[5px]`, `checked:bg-on-accent`, `h-[1lh]`), and a headless
Chromium screenshot of the rendered markup showed the 16 px ring and accent
fill lined up with the agent name. That is Chromium, not the app's WebKit, so
the checks below still need doing.

## 1. Settings → Notes, light mode

1. System Settings → Appearance → Light. Run the app (`just dev` on a dev
   machine) and open Settings → Notes ("Written by your own agent, on your
   own account").
2. Expected: each of Claude Code, Codex and "None, I'll copy the prompt" has
   a round radio about 16 px across. Unchecked ones are a grey 1.5 px ring.
   The picked one is a solid system-blue disc with a white centre and stands
   out at a glance.
3. Expected: the radio's centre is level with the agent name's line, not the
   middle of the name + detail block.

## 2. Settings → Notes, dark mode

1. Appearance → Dark, same screen.
2. Expected: the same shapes. The unchecked ring is light grey and plainly
   visible on the dark card. The checked one is the dark-mode system blue
   with a white centre.

## 3. Row click and keyboard

1. Click the detail line under "Codex" (not the dot). Expected: Codex is
   picked and saved, and the privacy sentence changes to OpenAI.
2. Click the empty space between an agent's name and its status pill.
   Expected: that agent is picked.
3. Press Tab until the picked radio has focus. Expected: a 2 px blue focus
   ring, round, around the radio.
4. Press Down and Up. Expected: the pick moves between the three choices and
   the focus ring follows.
5. Hover over an unchecked row. Expected: its ring darkens a little, and the
   pointer is a hand over the whole name and detail area.

## 4. Onboarding → agent step, light and dark

1. Start onboarding (fresh config) and go to the last step, "Who writes your
   notes".
2. Expected: the card looks exactly like Settings → Notes in both
   appearances (it is the same `AgentSetup` component).

## 5. Increase Contrast

1. Accessibility → Display → Increase Contrast on.
2. Expected: unchecked rings use the primary text colour, so they look black
   in light mode and white in dark mode.

## Other radios

`type="radio"` and `role="radio"` appear nowhere else in `src/`, so nothing
else needed the fix. TUR-75 (Speech engine picker) is meant to use
`src/ui/Radio.tsx`.
