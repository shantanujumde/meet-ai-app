/**
 * One meeting's notes run (TUR-10): where it is, what it wrote, and the two
 * things the user can do about it — start it (Retry) and cancel it.
 *
 * Rust starts the run on its own when a recording stops, so most of what this
 * shows arrives as status events rather than as answers to a click. Three
 * things keep a slow answer from showing the wrong thing:
 *
 * * **Everything is keyed by meeting.** State is stored with the meeting it
 *   belongs to and read back only for the current one, so switching meetings
 *   never shows the last one's run, even for a render.
 * * **An event beats an answer that was asked for before it.** The status
 *   fetch on open, or a Start that answers "running", must not overwrite a
 *   "done" event that came in while it was on its way.
 * * **Only the newest notes fetch lands.** Opening the meeting and a run
 *   finishing both fetch the notes; the older answer is dropped.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import {
  cancelNotesRun,
  meetingNotes,
  notesRunStatus,
  onNotesRunStatus,
  startNotesRun,
} from "@/ipc/client";
import type { MeetingNotes, NotesRunState, NotesRunStatus, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

/** What a meeting shows when its notes could not be read: nothing written yet. */
const NO_NOTES: MeetingNotes = { notesOff: false, analyzedBy: null, sections: [] };

export type NotesRun = {
  /** Null until the first answer for this meeting arrives. */
  state: NotesRunState | null;
  /** Null until the first answer for this meeting arrives. */
  notes: MeetingNotes | null;
  /** A Start or Cancel is on its way to Rust. */
  busy: boolean;
  /** Why the last Start or Cancel itself failed (not why the run failed). */
  error: UiError | null;
  /** Start the run. Ignored while one is running or a request is on its way. */
  start: () => void;
  /** Cancel the running run. Ignored while a request is on its way. */
  cancel: () => void;
};

type ForMeeting<T> = { meetingId: string; value: T };

export function useNotesRun(meetingId: string, onDone?: () => void): NotesRun {
  const [status, setStatus] = useState<NotesRunStatus | null>(null);
  const [notes, setNotes] = useState<ForMeeting<MeetingNotes> | null>(null);
  const [busyFor, setBusyFor] = useState<string | null>(null);
  const [error, setError] = useState<ForMeeting<UiError> | null>(null);

  // The meeting on screen, or null once unmounted: every async answer checks
  // it before it is allowed to change anything.
  const current = useRef<string | null>(meetingId);
  // Counts this meeting's status events, so an answer can tell whether one
  // arrived while it was on its way.
  const heard = useRef(0);
  const notesFetch = useRef(0);
  const inFlight = useRef<string | null>(null);

  // Usually an inline arrow, so held in a ref rather than made a dependency.
  const notifyDone = useRef(onDone);
  notifyDone.current = onDone;

  const refreshNotes = useCallback(async (id: string) => {
    const ticket = ++notesFetch.current;
    let value: MeetingNotes;
    try {
      value = await meetingNotes(id);
    } catch {
      value = NO_NOTES;
    }
    if (current.current === id && notesFetch.current === ticket) {
      setNotes({ meetingId: id, value });
    }
  }, []);

  const apply = useCallback(
    (next: NotesRunStatus) => {
      setStatus(next);
      if (next.state.state === "done") {
        void refreshNotes(next.meetingId);
        notifyDone.current?.();
      }
    },
    [refreshNotes],
  );

  useEffect(() => {
    current.current = meetingId;
    const heardBefore = heard.current;

    const stop = onNotesRunStatus((next) => {
      if (next.meetingId !== meetingId) return;
      heard.current += 1;
      apply(next);
    });

    const settle = (answer: NotesRunStatus) => {
      if (current.current === meetingId && heard.current === heardBefore) setStatus(answer);
    };
    notesRunStatus(meetingId).then(settle, () =>
      // Rust could not say: treat it as no run since launch, which offers a
      // start button, and Start asks Rust again.
      settle({ meetingId, state: { state: "idle" } }),
    );
    void refreshNotes(meetingId);

    return () => {
      current.current = null;
      stop();
    };
  }, [meetingId, apply, refreshNotes]);

  const request = useCallback(
    async (run: (id: string) => Promise<NotesRunStatus>) => {
      const id = meetingId;
      if (inFlight.current === id) return;
      inFlight.current = id;
      setBusyFor(id);
      setError(null);
      const heardBefore = heard.current;
      try {
        const answer = await run(id);
        if (current.current === id && heard.current === heardBefore) apply(answer);
      } catch (thrown) {
        if (current.current === id) setError({ meetingId: id, value: toUiError(thrown) });
      } finally {
        if (inFlight.current === id) inFlight.current = null;
        if (current.current === id) setBusyFor(null);
      }
    },
    [meetingId, apply],
  );

  const state = status?.meetingId === meetingId ? status.state : null;
  const running = state?.state === "running";

  const start = useCallback(() => {
    if (!running) void request(startNotesRun);
  }, [running, request]);

  const cancel = useCallback(() => {
    void request(cancelNotesRun);
  }, [request]);

  return {
    state,
    notes: notes?.meetingId === meetingId ? notes.value : null,
    busy: busyFor === meetingId,
    error: error?.meetingId === meetingId ? error.value : null,
    start,
    cancel,
  };
}
