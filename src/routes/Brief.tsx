/**
 * The pre-meeting brief (TUR-32, U5): before a call, what was said last time
 * and what landed in the repo since.
 *
 * Opened as `/brief?title=<title>` from the reminder notification (TUR-30) and
 * the Today pane. Read-only: Rust finds the last meeting with the same title
 * through the index and runs `git log`; nothing here starts an agent.
 */

import { Clock, ExternalLink, GitCommitHorizontal } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { meetingBrief } from "@/ipc/client";
import type { MeetingBrief, PreviousMeeting, RepoCommits, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { formatRelativeDate } from "@/lib/format";
import { meetingPath } from "@/lib/routes";
import { IconSquare } from "@/ui/icons";
import { Button, ButtonRow, Card, Pill, Prose } from "@/ui/primitives";
import { Checking, EmptyState, ErrorState } from "@/ui/states";

export function Brief() {
  const [params] = useSearchParams();
  const title = (params.get("title") ?? "").trim();
  const [brief, setBrief] = useState<MeetingBrief | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  const load = useCallback(async () => {
    setError(null);
    setBrief(null);
    if (!title) return;
    try {
      setBrief(await meetingBrief(title));
    } catch (caught) {
      setError(toUiError(caught));
    }
  }, [title]);

  useEffect(() => {
    void load();
  }, [load]);

  if (!title) {
    return (
      <div className="page">
        <EmptyState
          title="No meeting picked"
          body="Open the brief from a meeting reminder or from Today to see what was said last time."
        />
      </div>
    );
  }

  return (
    <div className="page">
      <div className="page__header">
        <h1 className="page__title">Before {title}</h1>
      </div>
      {error ? (
        <ErrorState error={error} onRemedy={() => void load()} />
      ) : brief === null ? (
        <Checking label="Reading last time's notes…" />
      ) : brief.previous === null ? (
        <EmptyState
          title="Nothing from last time"
          body={`There is no earlier meeting called “${title}”. After this one, its notes show up here next time.`}
        />
      ) : (
        <>
          <LastTime previous={brief.previous} />
          {brief.commits ? <Commits commits={brief.commits} /> : null}
        </>
      )}
    </div>
  );
}

function LastTime({ previous }: { previous: PreviousMeeting }) {
  const navigate = useNavigate();
  return (
    <section className="section" aria-labelledby="last-time-heading">
      <h2 className="section__title flex items-center gap-4" id="last-time-heading">
        <IconSquare icon={Clock} />
        Last time · {formatRelativeDate(previous.date)}
      </h2>
      <Card>
        <Part heading="Summary" body={previous.summary} empty="No summary was written." />
        <Part heading="Decisions" body={previous.decisions} empty="No decisions were written." />
        <article className="flex flex-col gap-2">
          <h3 className="m-0 text-headline font-semibold">Open tickets</h3>
          {previous.openTickets.length === 0 ? (
            <Prose className="m-0">No tickets left open.</Prose>
          ) : (
            <ul className="flex flex-col gap-2">
              {previous.openTickets.map((ticket) => (
                <li key={ticket.id} className="flex items-center gap-4">
                  <span className="font-mono text-caption1 text-fg-tertiary">{ticket.id}</span>
                  <span className="text-callout text-fg-primary">{ticket.title}</span>
                  {ticket.status === "in_progress" ? <Pill tone="warn">In progress</Pill> : null}
                </li>
              ))}
            </ul>
          )}
        </article>
        <ButtonRow>
          <Button
            size="small"
            icon={ExternalLink}
            onClick={() => navigate(meetingPath(previous.id))}
          >
            Open last meeting
          </Button>
        </ButtonRow>
      </Card>
    </section>
  );
}

function Part({ heading, body, empty }: { heading: string; body: string | null; empty: string }) {
  return (
    <article className="flex flex-col gap-2">
      <h3 className="m-0 text-headline font-semibold">{heading}</h3>
      <Prose className={body ? "m-0 whitespace-pre-wrap text-fg-primary" : "m-0"}>
        {body ?? empty}
      </Prose>
    </article>
  );
}

function Commits({ commits }: { commits: RepoCommits }) {
  return (
    <section className="section" aria-labelledby="commits-heading">
      <h2 className="section__title flex items-center gap-4" id="commits-heading">
        <IconSquare icon={GitCommitHorizontal} />
        Commits since then
      </h2>
      <Card>
        <span className="font-mono text-caption1 text-fg-tertiary">{commits.repo}</span>
        {commits.commits.length === 0 ? (
          <Prose className="m-0">No new commits since the last meeting.</Prose>
        ) : (
          <ul className="flex flex-col gap-2">
            {commits.commits.map((commit) => (
              <li key={commit.hash} className="flex gap-4">
                <span className="font-mono text-caption1 text-fg-tertiary">{commit.hash}</span>
                <span className="text-callout text-fg-primary">{commit.subject}</span>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </section>
  );
}
