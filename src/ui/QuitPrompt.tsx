/**
 * "Stop recording and quit?" — asked in the window when ⌘Q or the menu-bar
 * Quit comes in while recording (TUR-76), so a meeting is never cut by a
 * stray keypress.
 *
 * Rust holds the quit, shows the window and sends `app://confirm-quit`.
 * **Stop and quit** tells Rust to go ahead: it stops the recording through
 * the normal stop path (the files are finished like the Stop button
 * finishes them) and exits. **Cancel**, Escape, or the recording ending
 * some other way closes the question and meet-ai keeps running.
 *
 * Cancel has the focus when it opens: Return on a destructive question must
 * not be the destructive answer. Tab stays inside the dialog while it is up.
 */

import { type KeyboardEvent, useEffect, useId, useRef, useState } from "react";
import { confirmQuit, onQuitConfirm } from "@/ipc/client";
import { useRecordingStore } from "@/state/recording";
import { Button, ButtonRow } from "./primitives";

export function QuitPrompt() {
  const [open, setOpen] = useState(false);
  const [quitting, setQuitting] = useState(false);
  const phase = useRecordingStore((state) => state.status.phase);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const bodyId = useId();

  useEffect(() => onQuitConfirm(() => setOpen(true)), []);

  // Nothing left to protect: the recording ended while the question was up.
  useEffect(() => {
    if (phase === "idle" && !quitting) setOpen(false);
  }, [phase, quitting]);

  useEffect(() => {
    if (open) cancelRef.current?.focus();
  }, [open]);

  if (!open) return null;

  const stopAndQuit = async () => {
    setQuitting(true);
    try {
      await confirmQuit();
    } catch {
      // The quit did not go through; leave the choice on screen.
      setQuitting(false);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && !quitting) {
      event.preventDefault();
      setOpen(false);
      return;
    }
    if (event.key !== "Tab") return;
    // Two buttons: keep Tab and Shift+Tab between them.
    const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button")];
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/30 p-6">
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        onKeyDown={onKeyDown}
        className="flex w-[min(26rem,100%)] flex-col gap-4 rounded-card border-[0.5px] border-rim bg-glass-raised px-6 py-5 text-fg-primary shadow-floating"
      >
        <h2 id={titleId} className="text-headline font-semibold">
          Stop recording and quit?
        </h2>
        <p id={bodyId} className="text-body text-fg-secondary">
          meet-ai is recording. Quitting stops the recording and saves what was captured so far.
        </p>
        <ButtonRow className="justify-end">
          <Button ref={cancelRef} size="small" disabled={quitting} onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button
            tone="primary"
            size="small"
            disabled={quitting}
            onClick={() => void stopAndQuit()}
            className="bg-danger not-disabled:hover:bg-danger"
          >
            {quitting ? "Stopping…" : "Stop and quit"}
          </Button>
        </ButtonRow>
      </div>
    </div>
  );
}
