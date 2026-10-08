/**
 * A meeting's suggested tasks (TUR-113): what the notes run found, for the
 * user to Approve (it moves to Tickets) or Discard (dropped, its number never
 * reused). Approve all approves every one still waiting.
 *
 * An approved task stays listed here, marked Approved, with a link to Tickets
 * and, once sent, its issue key. Sending happens on the Tickets page's side,
 * on its own; this page has no Sync buttons.
 *
 * Renders nothing for a meeting with no tasks.
 */

import { Check, ListTodo, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router";
import {
  approveAllTasks,
  approveTask,
  discardTask,
  meetingTasks,
  onMeetingsChanged,
} from "@/ipc/client";
import type { TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { TICKETS } from "@/lib/routes";
import { IconSquare } from "./icons";
import { Button, ButtonRow, Card, Pill, Row } from "./primitives";
import { sentLabel } from "./SyncButton";
import { InlineError } from "./states";
import { TicketDetails } from "./TicketDetails";

const HINT =
  "Your agent found these in the meeting. Approve the ones you want to keep: they move to Tickets. Discard the rest.";
const ALL_HANDLED = "All tasks handled. Approved ones are in Tickets.";

export function MeetingTasks({ meetingId }: { meetingId: string }) {
  const [tasks, setTasks] = useState<TicketSummary[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  // The task an Approve or Discard is running for, or "all".
  const [busy, setBusy] = useState<string | null>(null);
  const [rowErrors, setRowErrors] = useState<Record<string, UiError>>({});

  const load = useCallback(async () => {
    try {
      setTasks(await meetingTasks(meetingId));
      setError(null);
    } catch (caught) {
      setError(toUiError(caught));
    }
  }, [meetingId]);

  useEffect(() => {
    setTasks(null);
    void load();
    // A notes run writing tasks, or a send updating one, changes the folder.
    return onMeetingsChanged(() => void load());
  }, [load]);

  async function act(key: string, run: () => Promise<void>) {
    setBusy(key);
    setRowErrors(({ [key]: _gone, ...rest }) => rest);
    try {
      await run();
    } catch (caught) {
      setRowErrors((all) => ({ ...all, [key]: toUiError(caught) }));
    } finally {
      setBusy(null);
    }
  }

  const approve = (task: TicketSummary) =>
    act(task.id, async () => {
      const approved = await approveTask(meetingId, task.id);
      setTasks(
        (current) => current?.map((each) => (each.id === task.id ? approved : each)) ?? null,
      );
    });

  const discard = (task: TicketSummary) =>
    act(task.id, async () => {
      await discardTask(meetingId, task.id);
      setTasks((current) => current?.filter((each) => each.id !== task.id) ?? null);
    });

  const approveAll = () =>
    act("all", async () => {
      setTasks(await approveAllTasks(meetingId));
    });

  if (error && tasks === null) {
    return (
      <section className="section" aria-labelledby="tasks-heading">
        <h2 className="section__title" id="tasks-heading">
          Suggested tasks
        </h2>
        <InlineError error={error} />
      </section>
    );
  }

  if (tasks === null || tasks.length === 0) return null;

  const waiting = tasks.filter((task) => task.suggested).length;

  return (
    <section className="section" aria-labelledby="tasks-heading">
      <div className="section__header">
        <h2 className="section__title" id="tasks-heading">
          Suggested tasks
        </h2>
        <p className="section__hint">{waiting === 0 ? ALL_HANDLED : HINT}</p>
        {waiting > 0 ? (
          <Button
            size="small"
            icon={Check}
            disabled={busy !== null}
            onClick={() => void approveAll()}
          >
            Approve all
          </Button>
        ) : null}
      </div>
      {rowErrors.all ? <InlineError error={rowErrors.all} /> : null}
      <Card flush>
        <ul>
          {tasks.map((task) => (
            <li key={task.id} className="[&+&]:border-t-[0.5px] [&+&]:border-separator">
              <Row divided={false} className="items-start gap-4">
                <span className="flex min-w-0 flex-1 items-start gap-5">
                  <IconSquare icon={ListTodo} />
                  <span className="flex min-w-0 flex-1 flex-col gap-1">
                    <span className="wrap-anywhere text-body font-semibold text-fg-primary">
                      {task.title}
                    </span>
                    <span className="wrap-anywhere font-mono text-caption1 text-fg-secondary">
                      {task.id}
                    </span>
                    <TicketDetails ticket={task} />
                  </span>
                </span>
                {task.suggested ? (
                  <ButtonRow className="shrink-0">
                    <Button
                      size="small"
                      icon={Check}
                      disabled={busy !== null}
                      aria-label={`Approve ${task.id}`}
                      onClick={() => void approve(task)}
                    >
                      Approve
                    </Button>
                    <Button
                      size="small"
                      tone="quiet"
                      icon={Trash2}
                      disabled={busy !== null}
                      aria-label={`Discard ${task.id}`}
                      onClick={() => void discard(task)}
                    >
                      Discard
                    </Button>
                  </ButtonRow>
                ) : (
                  <span className="flex shrink-0 flex-col items-end gap-2">
                    <Pill tone="ok">
                      {task.syncedTo !== null ? `Approved · ${sentLabel(task)}` : "Approved"}
                    </Pill>
                    <Link className="text-footnote text-accent-text" to={TICKETS}>
                      See in Tickets
                    </Link>
                  </span>
                )}
              </Row>
              {/* TUR-112: an error sits under the row at full width. */}
              {rowErrors[task.id] ? (
                <div className="px-(--card-pad-x) pb-(--row-pad-y)">
                  <InlineError error={rowErrors[task.id] as UiError} />
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      </Card>
    </section>
  );
}
