# Manual checks: TUR-112 (sync error layout)

Skipped here because jsdom has no layout; needs the running app.

1. Review screen, Tasks section: make a task's sync fail with a long message
   (e.g. sign out of the Linear MCP). Expected: title and ticket id keep
   normal width, Retry/Dismiss stay on the right of the title, the red error
   shows under the row at full card width and wraps inside the card; nothing
   overlaps.
2. Tickets screen: same failure. Expected: the error shows under the Start
   Work / Retry buttons at the card's width and wraps.
