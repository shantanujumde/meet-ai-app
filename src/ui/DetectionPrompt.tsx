/**
 * "Record this meeting?" — the in-app half of detection's one prompt path
 * (TUR-27).
 *
 * Rust notices a meeting (a meeting app opening, for now), posts a system
 * notification saying why, and sends the same prompt here on
 * `detection://prompt`. macOS notifications from the plugin cannot carry
 * buttons, so this banner is where the user answers: **Record** starts a
 * recording through the same command as the record button, **Dismiss** closes
 * it. Rust has already counted the prompt, so neither answer is sent back —
 * the app does not ask again until it quits and reopens.
 *
 * Detection never records on its own (L15): nothing here runs without a click.
 * A recording that starts any other way (the button, ⌘⇧R, the menu bar)
 * answers the question too, so the banner goes away.
 */

import { useEffect, useState } from "react";
import { useLocation } from "react-router";
import { onDetectionPrompt, type DetectionPrompt as Prompt } from "@/ipc/client";
import { isOnboardingPath } from "@/lib/routes";
import { useRecordingStore } from "@/state/recording";
import { Button, ButtonRow } from "./primitives";

export function DetectionPrompt() {
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const phase = useRecordingStore((state) => state.status.phase);
  const busy = useRecordingStore((state) => state.busy);
  // Onboarding owns the whole window, the same as the shell's record control.
  const onboarding = isOnboardingPath(useLocation().pathname);

  useEffect(() => onDetectionPrompt(setPrompt), []);

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
        <Button size="small" onClick={() => setPrompt(null)}>
          Dismiss
        </Button>
      </ButtonRow>
    </section>
  );
}
