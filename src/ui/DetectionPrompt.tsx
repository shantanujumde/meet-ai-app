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
 * Detection never records on its own (L15): nothing here runs without a click.
 * A recording that starts any other way (the button, ⌘⇧R, the menu bar)
 * answers the question too, so the banner goes away.
 */

import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { onDetectionPrompt, type DetectionPrompt as Prompt } from "@/ipc/client";
import { briefPath, isOnboardingPath } from "@/lib/routes";
import { useRecordingStore } from "@/state/recording";
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

  const record = () => {
    setPrompt(null);
    // Re-read at the click, not the render: `toggle` stops a running
    // recording, and Record must only ever start one.
    const recorder = useRecordingStore.getState();
    if (recorder.status.phase === "idle") void recorder.toggle();
  };

  const briefTitle = prompt.signal.kind === "calendar" ? prompt.signal.title : null;

  return (
    <section
      aria-labelledby="detection-prompt-title"
      aria-live="polite"
      className="fixed right-6 bottom-6 z-50 flex w-[min(24rem,calc(100vw-3rem))] flex-col gap-3 rounded-card border-[0.5px] border-rim bg-glass-raised px-5 py-4 text-fg-primary shadow-floating"
    >
      <h2 id="detection-prompt-title" className="text-headline font-semibold">
        Record this meeting?
      </h2>
      <p className="text-body text-fg-secondary">{prompt.reason}</p>
      <ButtonRow>
        <Button tone="primary" size="small" disabled={busy} onClick={record}>
          Record
        </Button>
        {briefTitle !== null && (
          <Button size="small" onClick={() => navigate(briefPath(briefTitle))}>
            Open brief
          </Button>
        )}
        <Button size="small" onClick={() => setPrompt(null)}>
          Dismiss
        </Button>
      </ButtonRow>
    </section>
  );
}
