/**
 * The "Make notes for this meeting" switch (TUR-12, SPEC A11 "Skip one
 * meeting"), for private calls.
 *
 * On by default. Off writes `agent_notes: off` into `meeting.md`, and from
 * then on nothing about this meeting is sent to the agent — including at Stop,
 * when it was switched off mid-recording; Rust makes sure of that. It sits at
 * the top of the meeting's notes section (TUR-81), which shows while
 * recording and after.
 *
 * Laid out as a Settings row: the label with its helper text under it on the
 * left, the switch on the right. The caller puts it in a flush `Card`.
 *
 * A real switch: a button with `role="switch"` and `aria-checked`, named by
 * its visible label (a `<label>` for a button, so clicking the words flips it
 * too). On and off differ in the knob's side as well as the fill, never by
 * colour alone. The "on" fill is green rather than the accent: the accent is
 * kept for the one primary control per window (Record, or Stop).
 */

import { useId } from "react";
import type { UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Row, rowDetailVariants } from "./primitives";
import { ErrorState } from "./states";

export const NOTES_SWITCH_LABEL = "Make notes for this meeting";

export function NotesSwitch({
  on,
  busy,
  error,
  onChange,
}: {
  on: boolean;
  /** A switch is on its way to Rust: the control waits for the answer. */
  busy: boolean;
  /** Why the last switch could not be saved. */
  error: UiError | null;
  onChange: (on: boolean) => void;
}) {
  const id = useId();
  const hintId = `${id}-hint`;

  return (
    <Row stacked>
      <Row bare divided={false}>
        <span className="flex min-w-0 flex-col gap-1">
          <label htmlFor={id} className="text-body font-medium">
            {NOTES_SWITCH_LABEL}
          </label>
          <span id={hintId} className={rowDetailVariants({ mono: false })}>
            {on
              ? "On: your agent writes notes from the transcript when the call ends."
              : "Off: the transcript is not sent to your agent. For private calls."}
          </span>
        </span>
        <button
          type="button"
          role="switch"
          id={id}
          aria-checked={on}
          aria-describedby={hintId}
          disabled={busy}
          onClick={() => onChange(!on)}
          className={cn(
            "inline-flex h-(--control-h-small) w-[36px] shrink-0 cursor-default items-center rounded-capsule p-1",
            "border-[0.5px] [transition:background-color_var(--dur-fast)_var(--ease-out)]",
            "disabled:cursor-not-allowed disabled:opacity-40",
            "contrast-more:border contrast-more:border-separator-strong",
            on ? "border-transparent bg-success" : "border-rim bg-glass-sunken",
          )}
        >
          <span
            aria-hidden="true"
            className={cn(
              "size-6 rounded-capsule bg-on-accent shadow-raised",
              "[transition:translate_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
              on ? "translate-x-6" : "translate-x-0",
            )}
          />
        </button>
      </Row>
      {error ? <ErrorState error={error} /> : null}
    </Row>
  );
}
