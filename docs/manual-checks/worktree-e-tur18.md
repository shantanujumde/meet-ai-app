# e-tur18 manual checks (TUR-18)

The fix is covered by headless tests in `src-tauri/src/tickets.rs`
(`cargo test -p meet-ai --lib tickets::`). One check in the running app is
left, because these runs cannot open the app:

1. **Hand-made ticket during a notes run.** With an agent CLI set up, end a
   short call so its notes start. While "Making notes" shows, open Tickets and
   add a ticket by hand.
   - Expected: the hand-made ticket gets a `TICK-NNNN` number none of the
     meeting's new tickets has, and it is one higher than the highest ticket
     anywhere under `~/Meetings/` (meeting tickets included).
   - Why skipped: needs the running app, a real call and a signed-in CLI.

## Behaviour notes

- Hand-made numbering now counts every meeting's tickets, not only the
  root `tickets/` folder. Before this change a hand-made ticket could take a
  number a meeting's ticket already had.
- A number a notes run keeps retired (its ticket deleted by the user, still
  named in that meeting's `agent_tickets` record) is skipped by hand-made
  tickets too. A deleted *hand-made* ticket has no record, so its number can
  still be handed out again, as before.
