# TUR-100: popup surface

Contrast (computed from token values, popup card composed over a black and a white backdrop; WCAG; border composed over the button fill over the card; the fill alone is only 1.06 to 1.09, so the edge carries the 3:1):

| Mode | Primary text | Secondary text | Dismiss/neutral border (`border-fg-secondary`) vs card |
|---|---|---|---|
| Light | 14.1 to 15.1 | 4.66 to 4.76 | 4.97 to 5.09 |
| Dark | 13.1 to 14.7 | 6.74 to 7.27 | 6.53 to 7.09 |

## Visual check over a busy backdrop (skipped: needs the running app)

- Run: a signed build on Windows/Linux (popup window), trigger a detection prompt with mic and speakers both in use.
- Put a bright web page, then a dark code editor, behind the popup; repeat in light and dark system mode.
- Expected: title, reason line and all buttons (Dismiss included) read clearly; Dismiss has a visible fill and edge.
- Also open Settings and the main window: `glass-raised` surfaces look unchanged.
- Note: the "ON THIS MAC" chip is not in `PromptPopup.tsx`; look for it if it shows in the popup.
- Why skipped: no display, no running app in this worktree.
