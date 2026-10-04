/**
 * "Record this meeting?" in its own small window (TUR-59): the popup that
 * Windows and Linux show for a detection or reminder prompt, in place of a
 * notification whose buttons those platforms never draw.
 *
 * Rust owns the prompt (`detection/popup`): it makes this window, sends the
 * prompt on `prompt-popup://show`, hides it after a while, and does what the
 * buttons say. **Record** starts a recording through the same paths as the
 * banner and the menu bar (a reminded meeting names itself from its invite),
 * **Dismiss** closes it, and a reminder with a meeting link adds **Join and
 * record** and **Join**, as TUR-78's banner. Nothing records without a click
 * (L15). A recording that starts any other way answers the question, so the
 * popup closes.
 */

import { Circle, Video, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { onRecordingState } from "@/ipc/client";
import {
  answerPromptPopup,
  onPromptPopup,
  type PopupAnswer,
  type PopupPrompt,
  promptPopupCurrent,
} from "@/ipc/promptPopup";
import { toUiError } from "@/ipc/types";
import { IconSquare } from "./icons";
import { Button, ButtonRow } from "./primitives";

/** The neutral buttons' default fill is glass, which vanishes on a solid card: give them a visible fill and edge. */
const QUIET_EDGE = "border-fg-secondary bg-glass-sunken";

export function PromptPopup() {
  const [shown, setShown] = useState<PopupPrompt | null>(null);
  const [error, setError] = useState<string | null>(null);
  const shownRef = useRef<PopupPrompt | null>(null);
  shownRef.current = shown;

  useEffect(() => {
    let live = true;
    promptPopupCurrent()
      .then((current) => {
        if (live && current !== null) setShown((now) => now ?? current);
      })
      .catch(() => {
        // Nothing on screen yet; the event below brings the prompt.
      });
    const stop = onPromptPopup((next) => {
      setError(null);
      setShown(next);
    });
    return () => {
      live = false;
      stop();
    };
  }, []);

  useEffect(
    () =>
      onRecordingState((status) => {
        if (status.phase === "idle") return;
        const now = shownRef.current;
        if (now === null) return;
        setShown(null);
        answerPromptPopup(now.id, "dismiss").catch(() => {
          // Rust hides it on its own timer anyway.
        });
      }),
    [],
  );

  if (shown === null) return null;
  const { id, prompt } = shown;

  const answer = (pressed: PopupAnswer) => {
    // Join opens the meeting and leaves the question up.
    if (pressed !== "join") setShown(null);
    answerPromptPopup(id, pressed).catch((thrown: unknown) => {
      setShown(shown);
      setError(toUiError(thrown).message);
    });
  };

  return (
    <section
      aria-labelledby="prompt-popup-title"
      aria-live="polite"
      className="flex h-screen flex-col gap-4 overflow-hidden rounded-card border-[0.5px] border-rim bg-popup px-5 py-4 text-fg-primary"
    >
      <div className="flex items-start gap-4">
        <IconSquare icon={Circle} />
        <div className="flex min-w-0 flex-col gap-1">
          <h2 id="prompt-popup-title" className="text-headline font-semibold">
            Record this meeting?
          </h2>
          <p className="line-clamp-2 text-body text-fg-secondary">{error ?? prompt.reason}</p>
        </div>
      </div>
      <ButtonRow>
        {prompt.canJoin ? (
          <>
            <Button
              tone="primary"
              size="small"
              icon={Video}
              onClick={() => answer("joinAndRecord")}
            >
              Join and record
            </Button>
            <Button size="small" icon={Video} className={QUIET_EDGE} onClick={() => answer("join")}>
              Join
            </Button>
            <Button
              size="small"
              icon={Circle}
              className={QUIET_EDGE}
              onClick={() => answer("record")}
            >
              Record
            </Button>
          </>
        ) : (
          <Button tone="primary" size="small" icon={Circle} onClick={() => answer("record")}>
            Record
          </Button>
        )}
        <Button size="small" icon={X} className={QUIET_EDGE} onClick={() => answer("dismiss")}>
          Dismiss
        </Button>
      </ButtonRow>
    </section>
  );
}
