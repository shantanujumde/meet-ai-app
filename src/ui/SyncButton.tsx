/**
 * A ticket's place in the tracker (TUR-11, TUR-113): where it is in being
 * sent, and why it was not.
 *
 * Rust sends tickets on its own (`useTicketSyncStates`); these pieces only
 * show the state and offer Retry.
 *
 * * {@link SendStatus} sits on the ticket's row: *Not sent*, *Sending to
 *   Linear…*, *In Linear: ENG-42* (opens the issue) or *Couldn't send to
 *   Linear*.
 * * {@link SendError} sits **under** the row, at full width, so a long
 *   message wraps there and never lies over the title (TUR-112). It carries
 *   Retry, and Open Tracker settings when the fix is in Settings.
 *
 * Rust opens the issue's address; the window never opens a URL itself.
 */

import { ExternalLink, RotateCcw, Settings as SettingsIcon, X } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router";
import { isKeptIssue, type SendState } from "@/hooks/useTicketSync";
import { dismissUnsavedSync, openSyncedIssue } from "@/ipc/client";
import { copyFor } from "@/ipc/errors";
import type { TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { settingsPath } from "@/lib/routes";
import { Button, ButtonRow, Pill } from "./primitives";
import { InlineError } from "./states";

const TRACKER_NAME: Record<string, string> = {
  linear: "Linear",
  jira: "Jira",
  github: "GitHub",
};

/** "Linear" for `linear`; the id itself for a tracker this window does not know. */
export function trackerName(tracker: string | null): string {
  return tracker ? (TRACKER_NAME[tracker] ?? tracker) : "your tracker";
}

/** "In Linear: ENG-42", or "In Linear" when the tracker gave no key. */
export function sentLabel(ticket: TicketSummary): string {
  const where = `In ${trackerName(ticket.syncedTo)}`;
  return ticket.externalId ? `${where}: ${ticket.externalId}` : where;
}

/** Words for a state, as the Tickets page shows them. */
export function sendLabel(state: SendState, tracker: string, ticket: TicketSummary): string {
  const name = trackerName(tracker);
  switch (state.kind) {
    case "sending":
      return `Sending to ${name}…`;
    case "sent":
      return sentLabel(ticket);
    case "failed":
      return `Couldn't send to ${name}`;
    default:
      return "Not sent";
  }
}

export function SendStatus({
  ticket,
  state,
  tracker,
  className,
}: {
  ticket: TicketSummary;
  state: SendState;
  /** The tracker set up in Settings, for the words before the ticket is sent. */
  tracker: string;
  className?: string;
}) {
  const [error, setError] = useState<UiError | null>(null);
  const label = sendLabel(state, tracker, ticket);

  async function open() {
    setError(null);
    try {
      await openSyncedIssue(ticket.id, ticket.meeting);
    } catch (caught) {
      setError(toUiError(caught));
    }
  }

  return (
    <div className={cn("flex flex-col items-end gap-2", className)}>
      {state.kind === "sent" && ticket.externalUrl ? (
        <Button
          size="small"
          icon={ExternalLink}
          aria-label={`${label}, open the issue`}
          onClick={() => void open()}
        >
          {label}
        </Button>
      ) : (
        <Pill tone={TONE[state.kind]} aria-busy={state.kind === "sending"}>
          {label}
        </Pill>
      )}
      {error ? <InlineError error={error} /> : null}
    </div>
  );
}

const TONE: Record<SendState["kind"], "neutral" | "ok" | "warn" | "danger"> = {
  "not-sent": "neutral",
  sending: "warn",
  sent: "ok",
  failed: "danger",
};

/**
 * Why a ticket was not sent, under its row: the message (it wraps), Retry,
 * and Open Tracker settings when the fix is there. Nothing for any other state.
 */
export function SendError({
  ticket,
  state,
  onRetry,
  className,
}: {
  ticket: TicketSummary;
  state: SendState;
  onRetry: () => void;
  className?: string;
}) {
  const navigate = useNavigate();
  const [dismissError, setDismissError] = useState<UiError | null>(null);
  if (state.kind !== "failed") return null;
  const inSettings = copyFor(state.error).remedy.action === "open-tracker-settings";
  const kept = isKeptIssue(state.error);

  async function dismiss() {
    setDismissError(null);
    try {
      await dismissUnsavedSync(ticket.id, ticket.meeting);
      onRetry();
    } catch (caught) {
      setDismissError(toUiError(caught));
    }
  }

  return (
    <div className={cn("flex w-full min-w-0 flex-col gap-3", className)}>
      <InlineError error={dismissError ?? state.error} />
      <ButtonRow>
        <Button
          size="small"
          icon={RotateCcw}
          aria-label={`Retry sending ${ticket.id}`}
          onClick={onRetry}
        >
          Retry
        </Button>
        {inSettings ? (
          <Button
            size="small"
            tone="quiet"
            icon={SettingsIcon}
            onClick={() => navigate(settingsPath("tracker"))}
          >
            Open Tracker settings
          </Button>
        ) : null}
        {kept ? (
          <Button
            size="small"
            tone="quiet"
            icon={X}
            aria-label={`Dismiss the unsaved issue of ${ticket.id}`}
            onClick={() => void dismiss()}
          >
            Dismiss
          </Button>
        ) : null}
      </ButtonRow>
    </div>
  );
}
