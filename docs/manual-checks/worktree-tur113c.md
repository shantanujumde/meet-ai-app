# TUR-113c: suggested tasks, Tickets sending status, readable errors (UI)

All skipped here: each needs the running signed app, a real meeting, and a signed-in agent with a tracker connection. Vitest covers the rows, the Tickets page with and without a tracker, the status labels and the error row's structure; jsdom has no layout, so the visual parts below are checked by hand.

## Record, approve, discard

- Run: a signed build. Record a short meeting that names three tasks; let the notes run finish.
- Expected: the meeting page shows **Suggested tasks** with the hint, each row with title, a two-line description (click expands it), owner and due when said, **Approve** and **Discard**; **Approve all** in the header.
- Approve two, Discard one. Expected: the two stay listed with an **Approved** badge and "See in Tickets"; no buttons on them; the discarded row is gone; the hint reads "All tasks handled. Approved ones are in Tickets." and Approve all is gone.
- Open Tickets. Expected: both tickets, each with "From: <meeting title>" (opens the meeting), its description, owner and due.

## Sending to Linear

- With no tracker saved: Tickets shows the "They stay on this Mac until you connect a tracker." hint and **Set up a tracker**, which opens Settings scrolled to Tracker. Every ticket reads *Not sent*, no error.
- In Settings, Tracker: save Linear and "claude.ai Linear", press **Send a test ticket**. Expected: a plain sentence such as "Claude Code reached Linear. New tickets will go to …"; nothing is created in Linear.
- Back in Tickets: each ticket goes *Sending to Linear…*, then *In Linear: KEY*. Pressing it opens the issue; the issue body has the ticket's description. The meeting page badge reads "Approved · In Linear: KEY".

## Error layout

- Disconnect "claude.ai Linear" in Claude Code, add a new ticket.
- Expected: *Couldn't send to Linear*, and under the card's line, at full width, the "couldn't reach Linear" message wrapping onto more lines, never over the title; **Retry** and **Open Tracker settings** below it. Try a narrow window too.
- Sign Claude Code out (`claude` → `/logout`), press Retry. Expected: the "isn't signed in" message with Retry only.
