/**
 * Sync: ask the user's agent CLI to create a ticket's issue in their tracker
 * (Linear, Jira or GitHub) through its MCP server (TUR-11).
 *
 * Two pieces:
 *
 * * {@link SyncControls} draws one ticket's controls from a {@link SyncState}
 *   it is given. A list that syncs several tickets (Sync all) holds the
 *   states itself, with `useTicketSync`, and draws each row with this.
 * * {@link SyncButton} is the same controls with their own state, for a
 *   screen that syncs tickets one by one.
 *
 * A synced ticket shows its issue id and an Open button instead. Rust opens
 * the issue's address; the window never opens a URL itself.
 */

import { ExternalLink, RefreshCw, RotateCcw, X } from "lucide-react";
import { useState } from "react";
import { openSyncedIssue } from "@/ipc/client";
import type { TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Button, ButtonRow, Pill } from "./primitives";
import { InlineError } from "./states";
import { isKeptIssue, type SyncState, useTicketSync } from "./useTicketSync";

const TRACKER_NAME: Record<string, string> = {
  linear: "Linear",
  jira: "Jira",
  github: "GitHub",
};

/** "Open in Linear", or "Open issue" for a tracker this window does not know. */
export function openLabel(syncedTo: string | null): string {
  const name = syncedTo ? TRACKER_NAME[syncedTo] : undefined;
  return name ? `Open in ${name}` : "Open issue";
}

export function SyncControls({
  ticket,
  state,
  onSync,
  onCancel,
  onDismiss,
  meetingId = ticket.meeting,
  canSync = true,
  disabled = false,
  className,
}: {
  ticket: TicketSummary;
  state: SyncState;
  onSync: () => void;
  onCancel: () => void;
  /**
   * Forget the issue Rust kept after a sync that could not attach it. The
   * Dismiss button shows only when this is given and the error is such a kept issue.
   */
  onDismiss?: () => void;
  /** The meeting whose folder holds the ticket. Defaults to `ticket.meeting`. */
  meetingId?: string | null;
  /** False when there is no agent to run: the Sync button is left out. */
  canSync?: boolean;
  /** Another sync is running, so this one cannot start yet. */
  disabled?: boolean;
  className?: string;
}) {
  if (ticket.syncedTo !== null) {
    return <SyncedIssue ticket={ticket} meetingId={meetingId} className={className} />;
  }
  if (!canSync) return null;

  const busy = state.kind === "busy";
  const failed = state.kind === "failed";
  const canDismiss = failed && onDismiss !== undefined && isKeptIssue(state.error);

  return (
    <div className={cn("flex flex-col gap-3", className)} aria-busy={busy}>
      <ButtonRow>
        {busy ? (
          <>
            <Button size="small" disabled>
              Syncing…
            </Button>
            <Button
              size="small"
              tone="quiet"
              disabled={state.cancelling}
              aria-label={`Cancel sync of ${ticket.id}`}
              onClick={onCancel}
            >
              {state.cancelling ? "Cancelling…" : "Cancel"}
            </Button>
          </>
        ) : (
          <Button
            size="small"
            icon={failed ? RotateCcw : RefreshCw}
            disabled={disabled}
            aria-label={failed ? `Retry sync of ${ticket.id}` : `Sync ${ticket.id}`}
            onClick={onSync}
          >
            {failed ? "Retry" : "Sync"}
          </Button>
        )}
        {canDismiss ? (
          <Button
            size="small"
            tone="quiet"
            icon={X}
            aria-label={`Dismiss the unsaved issue of ${ticket.id}`}
            onClick={onDismiss}
          >
            Dismiss
          </Button>
        ) : null}
        <span className="sr-only" role="status" aria-live="polite">
          {busy ? `Syncing ${ticket.id}` : ""}
        </span>
      </ButtonRow>
      {failed ? <InlineError error={state.error} /> : null}
      {busy && state.cancelError ? <InlineError error={state.cancelError} /> : null}
    </div>
  );
}

/** The issue a synced ticket became: its id and a button to open it. */
function SyncedIssue({
  ticket,
  meetingId,
  className,
}: {
  ticket: TicketSummary;
  meetingId: string | null;
  className?: string;
}) {
  const [error, setError] = useState<UiError | null>(null);

  async function open() {
    setError(null);
    try {
      await openSyncedIssue(ticket.id, meetingId);
    } catch (caught) {
      setError(toUiError(caught));
    }
  }

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      <ButtonRow>
        <Pill tone="ok">Synced</Pill>
        {ticket.externalId ? (
          <span className="font-mono text-caption1 text-fg-secondary">{ticket.externalId}</span>
        ) : null}
        {ticket.externalUrl ? (
          <Button size="small" icon={ExternalLink} onClick={() => void open()}>
            {openLabel(ticket.syncedTo)}
          </Button>
        ) : null}
      </ButtonRow>
      {error ? <InlineError error={error} /> : null}
    </div>
  );
}

/** One ticket's Sync, with its own state. */
export function SyncButton({
  ticket,
  onSynced,
  canSync = true,
  disabled = false,
  className,
}: {
  ticket: TicketSummary;
  onSynced: (ticket: TicketSummary) => void;
  canSync?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  const { stateOf, sync, cancel, dismiss } = useTicketSync(onSynced);
  return (
    <SyncControls
      ticket={ticket}
      state={stateOf(ticket.id)}
      onSync={() => void sync(ticket)}
      onCancel={() => void cancel(ticket.id)}
      onDismiss={() => void dismiss(ticket.id, ticket.meeting)}
      canSync={canSync}
      disabled={disabled}
      className={className}
    />
  );
}
