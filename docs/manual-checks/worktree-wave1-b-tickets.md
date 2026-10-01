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

## Results, 2026-10-01 (signed bundle, a throwaway meetings folder, driven by script)

- 1 ✅ No `tickets/` folder: "No tickets yet" empty state.
- 2 ✅ Create is dimmed with a blank title and a click on it writes nothing. A filled form writes `tickets/TICK-0001.md` with `status: open` and `meeting: ~` (YAML null), and the ticket shows at the top.
- 3 ✅ A `TICK-9999.md` with no frontmatter is listed under its file name, and the good ticket still shows.
- 4 ⏳ Not run: the move took about 2 s for 3.3 GB, too short to also open the form and create a ticket.

Bugs found:
- **The root `tickets/` folder shows up as a meeting.** After the first ticket, the sidebar and the meetings page list "tickets · No date · No transcript yet" and the count goes up by one.
- **Tickets made on this screen are not searchable.** The index reads tickets inside meeting folders (`crates/store/src/index.rs`, `folder.tickets`), but this screen writes to the root `tickets/` folder. A word only in TICK-0001 gives "No matches", even after a full index rebuild.

## Environment notes

- The disk was at 100% during the run (about 1 GB free), so `cargo test -p meet-ai` (the full one, which builds the static lib) failed with "No space left on device". `cargo test -p meet-ai --lib tickets` passed and clippy was clean before that.
