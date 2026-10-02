/**
 * The state behind every Sync button (TUR-11).
 *
 * One hook holds the sync state of many tickets, keyed by id, so a list can
 * run them one after another (Sync all) and still show each row's own
 * progress and error. A single Sync button is the same hook with one ticket.
 *
 * A sync is one run of the user's agent CLI and can take minutes. The window
 * only waits for Rust's answer: the updated ticket, or a `UiError` whose
 * message is shown as it is.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { cancelSync, syncTask, trackerSettings } from "@/ipc/client";
import type { TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

export type SyncState =
  | { kind: "idle" }
  | {
      kind: "busy";
      /** Cancel was pressed and Rust has not answered yet. */
      cancelling: boolean;
      /** Cancel itself failed; the run is still going. */
      cancelError: UiError | null;
    }
  | { kind: "failed"; error: UiError };

/** How one sync ended, for a caller running several in a row. */
export type SyncResult = "synced" | "failed" | "cancelled";

const IDLE: SyncState = { kind: "idle" };

export function useTicketSync(onSynced: (ticket: TicketSummary) => void) {
  const [states, setStates] = useState<Record<string, SyncState>>({});

  // The latest callback, so a sync that started several renders ago still
  // reports to the list as it is now.
  const latestOnSynced = useRef(onSynced);
  useLayoutEffect(() => {
    latestOnSynced.current = onSynced;
  });

  const put = useCallback((ticketId: string, state: SyncState) => {
    setStates((current) => ({ ...current, [ticketId]: state }));
  }, []);

  const updateBusy = useCallback(
    (ticketId: string, change: (busy: Extract<SyncState, { kind: "busy" }>) => SyncState) => {
      setStates((current) => {
        const state = current[ticketId];
        return state?.kind === "busy" ? { ...current, [ticketId]: change(state) } : current;
      });
    },
    [],
  );

  /** Sync one ticket. `meetingId` is the meeting whose folder holds it. */
  const sync = useCallback(
    async (ticket: TicketSummary, meetingId = ticket.meeting): Promise<SyncResult> => {
      put(ticket.id, { kind: "busy", cancelling: false, cancelError: null });
      try {
        const updated = await syncTask(ticket.id, meetingId);
        put(ticket.id, IDLE);
        latestOnSynced.current(updated);
        return "synced";
      } catch (caught) {
        const error = toUiError(caught);
        // The user asked for it, so it is not an error: back to plain Sync.
        if (error.domain === "app" && error.kind === "agent-cancelled") {
          put(ticket.id, IDLE);
          return "cancelled";
        }
        put(ticket.id, { kind: "failed", error });
        return "failed";
      }
    },
    [put],
  );

  /** Stop a running sync. Its `sync` call then resolves as `cancelled`. */
  const cancel = useCallback(
    async (ticketId: string) => {
      updateBusy(ticketId, (busy) => ({ ...busy, cancelling: true, cancelError: null }));
      try {
        await cancelSync(ticketId);
      } catch (caught) {
        const cancelError = toUiError(caught);
        updateBusy(ticketId, (busy) => ({ ...busy, cancelling: false, cancelError }));
      }
    },
    [updateBusy],
  );

  const stateOf = useCallback((ticketId: string) => states[ticketId] ?? IDLE, [states]);
  const anyBusy = Object.values(states).some((state) => state.kind === "busy");

  return { stateOf, sync, cancel, anyBusy };
}

/**
 * Whether there is an agent to run a sync: false when the user chose none.
 * Null until known, so a Sync button does not appear and then vanish. A
 * failed answer counts as yes — Rust then says what is wrong when Sync is
 * pressed, which beats a button that silently is not there.
 */
export function useCanSync(): boolean | null {
  const [canSync, setCanSync] = useState<boolean | null>(null);
  useEffect(() => {
    let current = true;
    trackerSettings().then(
      (settings) => {
        if (current) setCanSync(settings.harness !== "none");
      },
      () => {
        if (current) setCanSync(true);
      },
    );
    return () => {
      current = false;
    };
  }, []);
  return canSync;
}
