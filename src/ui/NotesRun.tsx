/**
 * The agent's notes for a meeting, and where the run that writes them is
 * (TUR-10).
 *
 * Rust starts the run on its own when a recording stops. This shows, in turn:
 * *Writing notes…* with Cancel; then either the notes, or why there are none
 * in plain words with Retry. A meeting with no run since launch (the app was
 * quit mid-run, say) shows what is on disk, and offers to start one when
 * there is nothing there.
 *
 * With notes switched off for the meeting (TUR-12) none of that shows: there
 * is no run to talk about. Switched back on, a meeting with no notes offers
 * **Make notes now** — including when its last run was refused because notes
 * were off, or cancelled by the switch.
 *
 * With notes set to run only when asked (TUR-101, `agent.auto_run` off),
 * nothing starts at Stop, and a meeting with no notes offers the same
 * **Make notes now** as its main state.
 *
 * The run itself is the page's {@link useNotesRun}, passed in, so the switch
 * and this pane read the same answers. The review screen passes the switch in
 * as `toggle` (TUR-81): with one, the section always shows — while recording
 * too, when the run panel itself waits for Stop.
 *
 * The sections are markdown as the agent wrote it, shown as pre-wrapped text:
 * readable as is, and no markdown library for four short sections.
 */

import { RotateCcw, Sparkles, X } from "lucide-react";
import type { ReactNode } from "react";
import type { NotesRun as NotesRunModel } from "@/hooks/useNotesRun";
import type {
  NotesRunFailure,
  NotesRunFailureKind,
  NotesRunState,
  NotesSection,
} from "@/ipc/types";
import { osText } from "@/lib/osText";
import { Button, ButtonRow, Card, Prose } from "./primitives";
import { ErrorState } from "./states";

/** A short title per failure. The sentence under it is Rust's own. */
const FAILURE_TITLES: Record<NotesRunFailureKind, string> = {
  "no-agent": "No agent is set up",
  "not-installed": "Your agent could not be found",
  "not-signed-in": "Your agent is not signed in",
  "timed-out": "Writing the notes took too long",
  cancelled: "You cancelled the notes",
  "cli-failed": "Your agent stopped with an error",
  "bad-reply": "Your agent's answer was not in the notes format",
  "could-not-start": "The notes could not be started",
  "no-transcript": "There is no transcript to write notes from",
  "notes-off": "Notes are off for this meeting",
  "write-failed": "The notes could not be saved",
};

/** `analyzed_by` as a person would say it. Anything else is shown as written. */
const AGENT_NAMES: Record<string, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  clipboard: "a copied prompt",
};

export function NotesRun({
  run,
  canStart,
  toggle,
  live = false,
}: {
  /** This meeting's run and notes, from the page's {@link useNotesRun}. */
  run: NotesRunModel;
  /**
   * Whether Retry and "Write notes" are offered: the meeting has a transcript,
   * and an agent is set up (with none, the header's Copy prompt stands in).
   */
  canStart: boolean;
  /**
   * The notes switch (and anything that goes with it), shown at the top of
   * the section. With it, the section is always there.
   */
  toggle?: ReactNode;
  /**
   * This meeting is recording: no run panel — the run starts on its own at
   * Stop, and its status event arrives then.
   */
  live?: boolean;
}) {
  const { state, notes, busy, error, start, cancel, switchedOn, manual } = run;

  // No run panel until both answers are in, so the page does not flash a
  // start button for a meeting that already has notes.
  const loaded = state !== null && notes !== null;
  if (!loaded && toggle === undefined) return null;

  const sections = notes?.sections ?? [];
  // Notes switched off for this meeting: nothing about runs at all.
  const panel =
    !loaded || live || notes.notesOff
      ? null
      : runPanel({
          state: afterSwitch(state, switchedOn),
          hasNotes: sections.length > 0,
          canStart,
          busy,
          justSwitchedOn: switchedOn,
          manual,
          start,
          cancel,
        });

  if (toggle === undefined && panel === null && sections.length === 0 && error === null) {
    return null;
  }

  const by = notes?.analyzedBy ?? null;
  return (
    <section className="section" aria-labelledby="meeting-notes-heading">
      <div className="section__header">
        <h2 className="section__title" id="meeting-notes-heading">
          Meeting notes
        </h2>
        <p className="section__hint" title="meeting.md in the meeting folder">
          {by ? `Written by ${AGENT_NAMES[by] ?? by}` : "Written by your agent"}
        </p>
      </div>
      {toggle}
      {panel}
      {error ? <ErrorState error={error} /> : null}
      {sections.length > 0 ? <Sections sections={sections} /> : null}
    </section>
  );
}

/**
 * A failure the switch explains, read as "no run yet" now notes are on: the
 * run refused because notes were off, or — just after switching back on —
 * the run the switch cancelled when it went off.
 */
function afterSwitch(state: NotesRunState, switchedOn: boolean): NotesRunState {
  if (state.state !== "failed") return state;
  const { kind } = state.failure;
  if (kind === "notes-off" || (switchedOn && kind === "cancelled")) return { state: "idle" };
  return state;
}

function runPanel({
  state,
  hasNotes,
  canStart,
  busy,
  justSwitchedOn,
  manual,
  start,
  cancel,
}: {
  state: NotesRunState;
  hasNotes: boolean;
  canStart: boolean;
  busy: boolean;
  /** Notes were just switched back on here: the user is asking for them. */
  justSwitchedOn: boolean;
  /** Notes run only when asked (TUR-101): starting them is the main state. */
  manual: boolean;
  start: () => void;
  cancel: () => void;
}): ReactNode {
  switch (state.state) {
    case "running":
      return (
        <RunCard
          status={
            <span className="checking">
              <span className="checking__dot" aria-hidden="true" />
              Writing notes…
            </span>
          }
          body="Your agent is reading the transcript. This can take a minute or two."
        >
          <Button size="small" icon={X} disabled={busy} onClick={cancel}>
            Cancel
          </Button>
        </RunCard>
      );
    case "failed":
      return <Failed failure={state.failure} canRetry={canStart} busy={busy} onRetry={start} />;
    case "done":
      return (
        <RunCard
          status={<span className="text-footnote text-fg-secondary">{doneLine(state.tasks)}</span>}
        />
      );
    case "idle":
      if (hasNotes || !canStart) return null;
      if (justSwitchedOn || manual) {
        return (
          <RunCard
            status={
              <span className="text-body font-medium">
                {justSwitchedOn ? "Notes are on for this meeting" : "Notes run when you ask"}
              </span>
            }
            body="Your agent can write a summary, the decisions and the tasks from this transcript."
          >
            <Button size="small" tone="primary" icon={Sparkles} disabled={busy} onClick={start}>
              Make notes now
            </Button>
          </RunCard>
        );
      }
      return (
        <RunCard
          status={<span className="text-body font-medium">No notes yet</span>}
          body="Your agent can write a summary, the decisions and the tasks from this transcript."
        >
          <Button size="small" tone="primary" icon={Sparkles} disabled={busy} onClick={start}>
            Write notes
          </Button>
        </RunCard>
      );
  }
}

function doneLine(tasks: number): string {
  if (tasks === 0) return "Notes written.";
  return `Notes written, with ${tasks === 1 ? "1 task" : `${tasks} tasks`} added to Tickets.`;
}

/**
 * The run's one card. `status` is the live region, so a screen reader hears
 * the run start, fail or finish without going looking; the buttons sit
 * outside it.
 */
function RunCard({
  status,
  body,
  children,
}: {
  status: ReactNode;
  body?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <Card className="gap-4">
      <div className="flex flex-col gap-2" role="status" aria-live="polite">
        {status}
        {body ? <Prose>{body}</Prose> : null}
      </div>
      {children ? <ButtonRow>{children}</ButtonRow> : null}
    </Card>
  );
}

function Failed({
  failure,
  canRetry,
  busy,
  onRetry,
}: {
  failure: NotesRunFailure;
  canRetry: boolean;
  busy: boolean;
  onRetry: () => void;
}) {
  return (
    <RunCard
      status={<span className="text-body font-medium">{FAILURE_TITLES[failure.kind]}</span>}
      body={failure.message}
    >
      {failure.command ? (
        <span className="flex flex-wrap items-center gap-3 text-footnote text-fg-secondary">
          Type this in {osText("terminal")}:
          <code className="select-all rounded-chip bg-glass-sunken px-3 py-1 font-mono text-caption1 text-fg-primary">
            {failure.command}
          </code>
        </span>
      ) : null}
      {canRetry ? (
        <Button size="small" icon={RotateCcw} disabled={busy} onClick={onRetry}>
          Retry
        </Button>
      ) : null}
    </RunCard>
  );
}

function Sections({ sections }: { sections: NotesSection[] }) {
  return (
    <div className="flex flex-col gap-6">
      {sections.map((section) => (
        <article key={section.heading} className="flex flex-col gap-2">
          <h3 className="m-0 text-headline font-semibold">{section.heading}</h3>
          <Prose className="m-0 whitespace-pre-wrap wrap-anywhere text-fg-primary">
            {section.body}
          </Prose>
        </article>
      ))}
    </div>
  );
}
