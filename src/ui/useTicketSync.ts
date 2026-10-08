/**
 * The send state of every ticket on the Tickets page (TUR-113).
 *
 * Rust sends each new ticket to the tracker on its own, one at a time
 * (`sync/auto.rs`). The window does not start sends: it reads where each one
 * is with `ticketSyncStates`, follows `TICKET_SYNC_EVENT` from then on, and
 * asks for a Retry when the user presses it.
 */

import { useCallback, useEffect, useState } from "react";
import {
  onTicketSync,
  retryTicketSync,
  type TicketSyncOverview,
  type TicketSyncStatus,
  ticketSyncStates,
} from "@/ipc/client";
import type { TicketSummary, Tracker, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

/**
 * The errors of a sync whose issue was created but not attached to the task.
 * Rust keeps that issue until the user dismisses it.
 */
const KEPT_ISSUE_KINDS: ReadonlySet<string> = new Set(["sync-not-saved", "sync-not-attached"]);

/** Whether this error is a kept issue the user can dismiss. */
export function isKeptIssue(error: UiError): boolean {
  return error.domain === "app" && KEPT_ISSUE_KINDS.has(error.kind);
}

/** One ticket's send state as the window shows it. */
export type SendState =
  | { kind: "not-sent" }
  | { kind: "sending" }
  | { kind: "sent" }
  | { kind: "failed"; error: UiError };

/** The state for a ticket: its issue key wins, then what Rust last said. */
export function sendStateOf(
  ticket: TicketSummary,
  status: TicketSyncStatus | undefined,
): SendState {
  if (ticket.externalId !== null || ticket.syncedTo !== null) return { kind: "sent" };
  if (!status) return { kind: "not-sent" };
  switch (status.state) {
    case "queued":
    case "sending":
      return { kind: "sending" };
    case "failed":
      return status.error ? { kind: "failed", error: status.error } : { kind: "not-sent" };
    default:
      return { kind: "not-sent" };
  }
}

export function useTicketSyncStates(onSent: (ticket: TicketSummary) => void) {
  const [overview, setOverview] = useState<TicketSyncOverview | null>(null);
  const [statuses, setStatuses] = useState<Record<string, TicketSyncStatus>>({});
  const [retryErrors, setRetryErrors] = useState<Record<string, UiError>>({});

  useEffect(() => {
    let current = true;
    const stop = onTicketSync((status) => {
      setStatuses((all) => ({ ...all, [status.ticketId]: status }));
      if (status.state === "sent" && status.ticket) onSent(status.ticket);
    });
    ticketSyncStates().then(
      (answer) => {
        if (!current) return;
        setOverview(answer);
        // Events that came in while this was on its way are newer: keep them.
        setStatuses((all) => ({
          ...Object.fromEntries(answer.tickets.map((each) => [each.ticketId, each])),
          ...all,
        }));
      },
      () => {
        // Unknown: every ticket reads Not sent and the hint stays out, rather
        // than claiming a tracker is or is not set up.
      },
    );
    return () => {
      current = false;
      stop();
    };
  }, [onSent]);

  const retry = useCallback(async (ticketId: string) => {
    setRetryErrors(({ [ticketId]: _gone, ...rest }) => rest);
    try {
      await retryTicketSync(ticketId);
    } catch (caught) {
      setRetryErrors((all) => ({ ...all, [ticketId]: toUiError(caught) }));
    }
  }, []);

  const stateOf = useCallback(
    (ticket: TicketSummary): SendState => {
      const retryError = retryErrors[ticket.id];
      if (retryError) return { kind: "failed", error: retryError };
      return sendStateOf(ticket, statuses[ticket.id]);
    },
    [statuses, retryErrors],
  );

  return {
    /** Null until Rust has answered. */
    trackerSetUp: overview?.trackerSetUp ?? null,
    tracker: (overview?.tracker ?? "linear") as Tracker,
    stateOf,
    retry,
  };
}
