/**
 * "Record this meeting?" — the in-app half of detection's one prompt path
 * (TUR-27).
 *
 * Rust notices a meeting (a meeting app opening, the mic and speakers both in
 * use, or a calendar invite starting in a minute), posts a system notification
 * saying why, and sends the same prompt here on `detection://prompt`. macOS
 * notifications from the plugin cannot carry buttons, so this banner is where
 * the user answers: **Record** starts a recording through the same command as
 * the record button (so a reminded meeting names itself from its invite,
 * TUR-29), **Dismiss** closes it. A calendar reminder (TUR-30) adds **Open
 * brief**, which shows that meeting's brief and leaves the banner up. Rust has
 * already counted the prompt, so no answer is sent back.
 *
 * A prompt with `updateOnly` is the same call noticed a second way (a reminder
 * right after Zoom opened): it replaces the banner on screen, and never brings
 * back one the user dismissed.
 *
 * TUR-78: a reminder for an event with a meeting link leads with **Join and
 * record** (opens the link and starts a recording named after the meeting),
 * then **Join**, **Record**, **Open brief** and **Dismiss**. A reminder's
 * Join and Record go through Rust with the event's id, never a link, so only
 * the calendar's own link is opened. A test reminder from Settings shows the
 * same buttons, and each one only closes it.
 *
 * Detection never records on its own (L15): nothing here runs without a click.
 * A recording that starts any other way (the button, ⌘⇧R, the menu bar)
 * answers the question too, so the banner goes away.
 */

import { Circle, Video, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import {
  joinRemindedMeeting,
  onDetectionPrompt,
  type DetectionPrompt as Prompt,
  recordRemindedMeeting,
} from "@/ipc/client";
import { toUiError } from "@/ipc/types";
import { briefPath, isOnboardingPath } from "@/lib/routes";
import { useRecordingStore } from "@/state/recording";
import { IconSquare } from "./icons";
import { Button, ButtonRow } from "./primitives";

export function DetectionPrompt() {
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const phase = useRecordingStore((state) => state.status.phase);
  const busy = useRecordingStore((state) => state.busy);
  const navigate = useNavigate();
  // Onboarding owns the whole window, the same as the shell's record control.
  const onboarding = isOnboardingPath(useLocation().pathname);

  useEffect(
    () =>
      onDetectionPrompt((next) =>
        setPrompt((current) => (next.updateOnly && current === null ? null : next)),
      ),
    [],
  );

  useEffect(() => {
    if (phase !== "idle") setPrompt(null);
  }, [phase]);

  if (prompt === null || phase !== "idle" || onboarding) return null;

  const eventId = prompt.eventId;
  const test = prompt.test;

  const record = () => {
    setPrompt(null);
    if (test) return;
    // Re-read at the click, not the render: `toggle` stops a running
    // recording, and Record must only ever start one.
    const recorder = useRecordingStore.getState();
    if (recorder.status.phase !== "idle") return;
    if (eventId === null) {
      void recorder.toggle();
      return;
    }
    void recordReminded(eventId, false);
  };

  const joinAndRecord = () => {
    setPrompt(null);
    if (test || eventId === null) return;
    if (useRecordingStore.getState().status.phase !== "idle") return;
    void recordReminded(eventId, true);
  };

  const join = () => {
    if (test || eventId === null) {
      setPrompt(null);
      return;
    }
    // Joining is not an answer to "record?": the banner stays.
    joinRemindedMeeting(eventId).catch((thrown: unknown) =>
      useRecordingStore.setState({ error: toUiError(thrown) }),
    );
  };

  const openBrief = (title: string) => {
    if (test) {
      setPrompt(null);
      return;
    }
    navigate(briefPath(title));
  };

  const briefTitle = prompt.signal.kind === "calendar" ? prompt.signal.title : null;

  return (
    <section
      aria-labelledby="detection-prompt-title"
      aria-live="polite"
      className="fixed right-6 bottom-6 z-50 flex w-[min(24rem,calc(100vw-3rem))] flex-col gap-5 rounded-(--card-radius) bg-card px-6 py-5 text-fg-primary shadow-floating contrast-more:border contrast-more:border-separator-strong"
    >
      <div className="flex items-start gap-5">
        <IconSquare icon={Circle} />
        <div className="flex min-w-0 flex-col gap-2">
          <h2 id="detection-prompt-title" className="text-headline font-semibold">
            Record this meeting?
          </h2>
          <p className="text-body text-fg-secondary">{prompt.reason}</p>
        </div>
      </div>
      <ButtonRow>
        {prompt.canJoin ? (
          <>
            <Button
              tone="primary"
              size="small"
              icon={Video}
              disabled={busy}
              onClick={joinAndRecord}
            >
              Join and record
            </Button>
            <Button size="small" icon={Video} onClick={join}>
              Join
            </Button>
            <Button size="small" icon={Circle} disabled={busy} onClick={record}>
              Record
            </Button>
          </>
        ) : (
          <Button tone="primary" size="small" icon={Circle} disabled={busy} onClick={record}>
            Record
          </Button>
        )}
        {briefTitle !== null && (
          <Button size="small" onClick={() => openBrief(briefTitle)}>
            Open brief
          </Button>
        )}
        <Button size="small" icon={X} onClick={() => setPrompt(null)}>
          Dismiss
        </Button>
      </ButtonRow>
    </section>
  );
}

/**
 * Start a recording named after the reminded meeting, joining it first when
 * `join`. Busy while it runs, like the record button; a refusal shows where
 * the record button's would.
 */
async function recordReminded(eventId: string, join: boolean) {
  const store = useRecordingStore;
  if (store.getState().busy) return;
  store.setState({ busy: true, error: null });
  try {
    await recordRemindedMeeting(eventId, join);
  } catch (thrown) {
    store.setState({ error: toUiError(thrown) });
  } finally {
    store.setState({ busy: false });
  }
}
