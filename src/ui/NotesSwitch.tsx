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
 * The switch is {@link Switch}, named by its label and described by the
 * helper text.
 */

import { useId } from "react";
import type { UiError } from "@/ipc/types";
import { Row, rowDetailVariants } from "./primitives";
import { Switch } from "./SettingSwitch";
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
        <Switch id={id} on={on} disabled={busy} describedBy={hintId} onChange={onChange} />
      </Row>
      {error ? <ErrorState error={error} /> : null}
    </Row>
  );
}
