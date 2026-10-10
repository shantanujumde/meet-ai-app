/**
 * The ticket list, and a small form to add one.
 *
 * TUR-113: every ticket in Tickets, the ones approved from a meeting (with a
 * "From: <meeting>" link) and the hand-made ones. When a tracker is set up,
 * Rust sends each new ticket to it on its own; each card shows where that is,
 * and a failure shows under the card's line with Retry.
 *
 * Local state rather than the global store: nothing else in the app reads
 * tickets, so the page loads them itself when it opens, and reads them again,
 * quietly, whenever the meetings folder changes (TUR-153). A meeting's
 * suggested tasks are not listed here until approved (SPEC A26).
 */

import { Plus, Settings as SettingsIcon, Ticket as TicketIcon } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router";
import { useTicketSyncStates } from "@/hooks/useTicketSync";
import { createTicket, listTickets, onMeetingsChanged, startWorkPrompt } from "@/ipc/client";
import type { TicketStatus, TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { osText } from "@/lib/osText";
import { meetingPath, settingsPath } from "@/lib/routes";
import { CopyPromptButton } from "@/ui/CopyPromptButton";
import { IconSquare } from "@/ui/icons";
import { Button, ButtonRow, cardVariants, Pill } from "@/ui/primitives";
import { SendError, SendStatus, trackerName } from "@/ui/SyncButton";
import { SETTINGS_FIELD as FIELD } from "@/ui/settings/SettingsSection";
import { Checking, EmptyState, ErrorState } from "@/ui/states";
import { TicketDetails } from "@/ui/TicketDetails";

const STATUS_LABEL: Record<TicketStatus, string> = {
  open: "Open",
  in_progress: "In progress",
  done: "Done",
  dropped: "Dropped",
};

const STATUS_TONE: Record<TicketStatus, "neutral" | "ok" | "warn" | "danger"> = {
  open: "neutral",
  in_progress: "warn",
  done: "ok",
  dropped: "danger",
};

const HINT_START = "Tickets you approved from meetings, and ones you added.";

export function Tickets() {
  const [tickets, setTickets] = useState<TicketSummary[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const navigate = useNavigate();
  const replace = useCallback((updated: TicketSummary) => {
    setTickets((current) =>
      (current ?? []).map((each) => (each.id === updated.id ? updated : each)),
    );
  }, []);
  const sync = useTicketSyncStates(replace);

  // Bumped by every read, and by a ticket added here, so only the newest
  // answer is shown: an older read landing last must not bring back a list
  // from before a change (TUR-153).
  const reads = useRef(0);
  const load = useCallback(async () => {
    reads.current += 1;
    const read = reads.current;
    try {
      const answer = await listTickets();
      if (read !== reads.current) return;
      setTickets(answer);
      setError(null);
    } catch (caught) {
      if (read !== reads.current) return;
      setError(toUiError(caught));
    }
  }, []);

  useEffect(() => {
    void load();
    // An approve on a meeting page, or a send writing the issue key, changes the folder.
    return onMeetingsChanged(() => void load());
  }, [load]);

  if (error && tickets === null) {
    return (
      <div className="page">
        <ErrorState error={error} onRemedy={() => void load()} />
      </div>
    );
  }

  if (tickets === null) {
    return (
      <div className="page">
        <Checking label="Reading your tickets…" />
      </div>
    );
  }

  const name = trackerName(sync.tracker);

  return (
    <div className="page">
      <div className="page__header items-start">
        <h1 className="page__title">Tickets</h1>
        {sync.trackerSetUp === true ? (
          <p className="text-callout text-fg-secondary">
            {`${HINT_START} Each one is sent to ${name} on its own.`}
          </p>
        ) : null}
        {sync.trackerSetUp === false ? (
          <>
            <p className="text-callout text-fg-secondary">
              {`${HINT_START} They stay on ${osText("thisComputer")} until you connect a tracker.`}
            </p>
            <Button
              size="small"
              icon={SettingsIcon}
              onClick={() => navigate(settingsPath("tracker"))}
            >
              Set up a tracker
            </Button>
          </>
        ) : null}
      </div>

      {formOpen ? (
        <NewTicketForm
          onCreated={(ticket) => {
            reads.current += 1;
            setTickets((current) => [ticket, ...(current ?? [])]);
            setFormOpen(false);
          }}
          onCancel={() => setFormOpen(false)}
        />
      ) : (
        <ButtonRow>
          <Button tone="primary" icon={Plus} onClick={() => setFormOpen(true)}>
            New ticket
          </Button>
        </ButtonRow>
      )}

      {tickets.length === 0 ? (
        <EmptyState
          title="No tickets yet"
          body="Approve a task on a meeting's page, or use New ticket to add one."
        />
      ) : (
        <ul className="flex flex-col gap-4">
          {tickets.map((ticket) => {
            const state = sync.stateOf(ticket);
            return (
              <li key={ticket.id} className={cardVariants()}>
                <div className="flex items-start gap-5">
                  <IconSquare icon={TicketIcon} />
                  <div className="flex min-w-0 flex-1 flex-col gap-2">
                    <div className="flex items-center justify-between gap-5">
                      <span className="font-mono text-caption1 text-fg-tertiary">{ticket.id}</span>
                      {ticket.status ? (
                        <Pill tone={STATUS_TONE[ticket.status]}>{STATUS_LABEL[ticket.status]}</Pill>
                      ) : null}
                    </div>
                    <h2 className="wrap-anywhere text-body font-semibold text-fg-primary">
                      {ticket.title}
                    </h2>
                    {ticket.meeting ? (
                      <Link
                        className="wrap-anywhere text-footnote text-accent-text"
                        to={meetingPath(ticket.meeting)}
                      >
                        {`From: ${ticket.meetingTitle ?? ticket.meeting}`}
                      </Link>
                    ) : null}
                    <TicketDetails ticket={ticket} />
                  </div>
                </div>
                {/* L14: Rust renders the prompt; the user pastes it into their
                    own agent session in the repo. */}
                <ButtonRow className="items-start justify-between">
                  <CopyPromptButton
                    label="Start Work"
                    size="small"
                    render={() => startWorkPrompt(ticket.id, ticket.meeting)}
                  />
                  <SendStatus ticket={ticket} state={state} tracker={sync.tracker} />
                </ButtonRow>
                {/* TUR-112: the error under the buttons, at the card's full width. */}
                <SendError
                  ticket={ticket}
                  state={state}
                  onRetry={() => void sync.retry(ticket.id)}
                />
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function NewTicketForm({
  onCreated,
  onCancel,
}: {
  onCreated: (ticket: TicketSummary) => void;
  onCancel: () => void;
}) {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);

  const canSubmit = title.trim().length > 0 && !busy;

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      onCreated(await createTicket(title.trim(), body));
    } catch (caught) {
      setError(toUiError(caught));
      setBusy(false);
    }
  }

  return (
    <form className={cardVariants()} onSubmit={(event) => void submit(event)}>
      <label className="flex flex-col gap-2 text-callout text-fg-secondary">
        Title
        <input
          className={FIELD}
          value={title}
          required
          onChange={(event) => setTitle(event.target.value)}
        />
      </label>
      <label className="flex flex-col gap-2 text-callout text-fg-secondary">
        Details
        <textarea
          className={FIELD}
          rows={4}
          value={body}
          onChange={(event) => setBody(event.target.value)}
        />
      </label>
      {error ? <ErrorState error={error} /> : null}
      <ButtonRow>
        <Button type="submit" tone="primary" icon={Plus} disabled={!canSubmit}>
          Create
        </Button>
        <Button onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
      </ButtonRow>
    </form>
  );
}
