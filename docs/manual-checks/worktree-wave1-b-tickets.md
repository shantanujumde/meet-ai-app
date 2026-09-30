# Manual checks: TUR-102 tickets screen

Skipped because they need the running app (a window). Everything else was tested headless.

1. **Open the Tickets screen.** Run the app, click "Tickets" in the sidebar (above Settings).
   Expected: with no `tickets/` folder in the meetings root, an empty state shows. Existing `TICK-NNNN.md` files are listed newest first, with id, status and a body preview.
   Why skipped: needs the app window.
2. **Make a ticket by hand.** Click "New ticket", type a title and details, press Create.
   Expected: the ticket appears at the top, and `<meetings root>/tickets/TICK-NNNN.md` exists with `status: open` and `meeting: null`. Create is disabled while the title is blank.
   Why skipped: needs the app window.
3. **Broken file.** Put a `tickets/TICK-9999.md` with no frontmatter in the folder, reopen the screen.
   Expected: it is listed, not hidden, and does not stop other tickets showing.
   Why skipped: needs the app window.
4. **Moving the meetings folder** while creating a ticket is refused by the same guard as saving notes. Not exercised by hand.

## Environment notes

- The disk was at 100% during the run (about 1 GB free), so `cargo test -p meet-ai` (the full one, which builds the static lib) failed with "No space left on device". `cargo test -p meet-ai --lib tickets` passed and clippy was clean before that.
