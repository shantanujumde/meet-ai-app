/**
 * The tasks that came out of one meeting, each with its Sync button, and
 * Sync all to send every unsynced one to the tracker (TUR-11).
 *
 * Sync all runs the tasks **one at a time**: each is its own agent run, and
 * each row shows its own progress and error as it goes. A failed row keeps
 * its error and Sync all moves on to the next; Cancel stops the whole batch.
 *
 * Renders nothing for a meeting with no tasks.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { meetingTasks, onMeetingsChanged } from "@/ipc/client";
import type { TicketSummary, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Button, Card } from "./primitives";
import { SyncControls } from "./SyncButton";
import { InlineError } from "./states";
import { useCanSync, useTicketSync } from "./useTicketSync";

export function MeetingTasks({ meetingId }: { meetingId: string }) {
  const [tasks, setTasks] = useState<TicketSummary[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [syncingAll, setSyncingAll] = useState(false);
  // Set by Cancel during Sync all, so the batch stops even if the cancelled
  // run happened to finish first.
  const stopAll = useRef(false);

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
    // An agent writing tickets, or a sync updating one, changes the folder.
    return onMeetingsChanged(() => void load());
  }, [load]);

  const replace = useCallback((updated: TicketSummary) => {
    setTasks(
      (current) => current?.map((task) => (task.id === updated.id ? updated : task)) ?? current,
    );
  }, []);

  const { stateOf, sync, cancel, anyBusy } = useTicketSync(replace);
  const canSync = useCanSync();

  async function syncAll(list: TicketSummary[]) {
    stopAll.current = false;
    setSyncingAll(true);
    for (const task of list.filter((each) => each.syncedTo === null)) {
      if (stopAll.current) break;
      const result = await sync(task, meetingId);
      if (result === "cancelled") break;
    }
    setSyncingAll(false);
  }

  function cancelTask(ticketId: string) {
    stopAll.current = true;
    void cancel(ticketId);
  }

  if (error && tasks === null) {
    return (
      <section className="section" aria-labelledby="tasks-heading">
        <h2 className="section__title" id="tasks-heading">
          Tasks
        </h2>
        <InlineError error={error} />
      </section>
    );
  }

  if (tasks === null || tasks.length === 0) return null;

  const unsynced = tasks.filter((task) => task.syncedTo === null).length;
  const busy = anyBusy || syncingAll;

  return (
    <section className="section" aria-labelledby="tasks-heading">
      <div className="section__header">
        <h2 className="section__title" id="tasks-heading">
          Tasks
        </h2>
        <p className="section__hint">
          {unsynced === 0 ? "All synced" : `${unsynced} not synced yet`}
        </p>
        {canSync && unsynced > 0 ? (
          <Button size="small" disabled={busy} onClick={() => void syncAll(tasks)}>
            Sync all
          </Button>
        ) : null}
      </div>
      <Card flush>
        <ul>
          {tasks.map((task) => (
            <li
              key={task.id}
              className="flex flex-col gap-3 px-6 py-5 [&+&]:border-t-[0.5px] [&+&]:border-separator"
            >
              <div className="flex min-w-0 items-baseline gap-4">
                <span className="font-mono text-caption1 text-fg-tertiary">{task.id}</span>
                <span className="text-body font-medium text-fg-primary">{task.title}</span>
              </div>
              <SyncControls
                ticket={task}
                state={stateOf(task.id)}
                meetingId={meetingId}
                canSync={canSync === true}
                disabled={busy}
                onSync={() => void sync(task, meetingId)}
                onCancel={() => cancelTask(task.id)}
              />
            </li>
          ))}
        </ul>
      </Card>
    </section>
  );
}
