/**
 * The ticket list, and a small form to add one.
 *
 * Local state rather than the global store: nothing else in the app reads
 * tickets, so the page loads them itself when it opens.
 */

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { createTicket, listTickets, startWorkPrompt } from "@/ipc/client";
import type { TicketStatus, TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { CopyPromptButton } from "@/ui/CopyPromptButton";
import { Button, ButtonRow, cardVariants, Pill } from "@/ui/primitives";
import { SyncButton } from "@/ui/SyncButton";
import { Checking, EmptyState, ErrorState } from "@/ui/states";
import { useCanSync } from "@/ui/useTicketSync";

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

const FIELD =
  "w-full rounded-control border-[0.5px] border-separator bg-glass-sunken px-4 py-3 text-body text-fg-primary";

export function Tickets() {
  const [tickets, setTickets] = useState<TicketSummary[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const canSync = useCanSync();

  const load = useCallback(async () => {
    setError(null);
    try {
      setTickets(await listTickets());
    } catch (caught) {
      setError(toUiError(caught));
    }
  }, []);

  useEffect(() => {
    void load();
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

  return (
    <div className="page">
      <div className="page__header">
        <h1 className="page__title">Tickets</h1>
      </div>

      {formOpen ? (
        <NewTicketForm
          onCreated={(ticket) => {
            setTickets((current) => [ticket, ...(current ?? [])]);
            setFormOpen(false);
          }}
          onCancel={() => setFormOpen(false)}
        />
      ) : (
        <ButtonRow>
          <Button tone="primary" onClick={() => setFormOpen(true)}>
            New ticket
          </Button>
        </ButtonRow>
      )}

      {tickets.length === 0 ? (
        <EmptyState
          title="No tickets yet"
          body="Tickets are small tasks that come out of your meetings. Use New ticket to add the first one."
        />
      ) : (
        <ul className="flex flex-col gap-4">
          {tickets.map((ticket) => (
            <li key={ticket.id} className={cardVariants()}>
              <div className="flex items-center justify-between gap-5">
                <span className="font-mono text-caption1 text-fg-tertiary">{ticket.id}</span>
                {ticket.status ? (
                  <Pill tone={STATUS_TONE[ticket.status]}>{STATUS_LABEL[ticket.status]}</Pill>
                ) : null}
              </div>
              <h2 className="text-body font-medium text-fg-primary">{ticket.title}</h2>
              {ticket.body ? (
                <p className="line-clamp-3 text-callout text-fg-secondary">{ticket.body}</p>
              ) : null}
              {/* L14: Rust renders the prompt; the user pastes it into their
                  own agent session in the repo. */}
              <ButtonRow className="items-start">
                <CopyPromptButton
                  label="Start Work"
                  size="small"
                  render={() => startWorkPrompt(ticket.id, ticket.meeting)}
                />
                {/* TUR-11: the agent creates the issue in the user's tracker. */}
                <SyncButton
                  ticket={ticket}
                  canSync={canSync === true}
                  onSynced={(updated) =>
                    setTickets((current) =>
                      (current ?? []).map((each) => (each.id === updated.id ? updated : each)),
                    )
                  }
                />
              </ButtonRow>
            </li>
          ))}
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
        <Button type="submit" tone="primary" disabled={!canSubmit}>
          Create
        </Button>
        <Button onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
      </ButtonRow>
    </form>
  );
}
