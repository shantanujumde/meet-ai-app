/**
 * Settings → When notes run (TUR-101): `agent.auto_run` as two choices.
 * "Automatically after the call" (the default) starts the notes run once the
 * transcript is final; "Only when I click" starts nothing, and the meeting
 * shows "Make notes now" instead. Settings only, not onboarding.
 */

import { Clock } from "lucide-react";
import { useId } from "react";
import { useSavedSetting } from "@/hooks/useIpcValue";
import { notesAutoRun, type SettingsSnapshot, saveNotesAutoRun } from "@/ipc/client";
import { rememberNotesAutoRun } from "@/state/session";
import { IconSquare } from "@/ui/icons";
import { Row, rowDetailVariants } from "@/ui/primitives";
import { Radio } from "@/ui/Radio";
import { SettingsSection } from "@/ui/settings/SettingsSection";
import { ErrorState } from "@/ui/states";
import { readOrThrow, useSnapshotLoad } from "../settings/snapshot";

export const NOTES_AUTO_LABEL = "Automatically after the call";
export const NOTES_MANUAL_LABEL = "Only when I click";

const CHOICES = [
  {
    value: "auto",
    on: true,
    name: NOTES_AUTO_LABEL,
    detail: "Notes start on their own once the transcript is ready",
  },
  {
    value: "manual",
    on: false,
    name: NOTES_MANUAL_LABEL,
    detail: "Nothing runs after the call. Open the meeting and click Make notes now.",
  },
] as const;

const pick = (snapshot: SettingsSnapshot) =>
  readOrThrow(snapshot.notesAutoRun, snapshot.agentError);

/** Saves, and keeps the session's answer in step (TUR-171). */
async function save(on: boolean): Promise<boolean> {
  const saved = await saveNotesAutoRun(on);
  rememberNotesAutoRun(saved);
  return saved;
}

export function NotesWhenSetting() {
  const group = useId();
  const load = useSnapshotLoad(pick, notesAutoRun);
  const { value: on, busy, error, change } = useSavedSetting(load, save);

  return (
    <SettingsSection title="When notes run" after={error ? <ErrorState error={error} /> : null}>
      <fieldset className="contents">
        <legend className="sr-only">When notes run</legend>
        <Row>
          <span className="flex min-w-0 items-center gap-5">
            <IconSquare icon={Clock} />
            <span className="text-body font-semibold">After a call</span>
          </span>
        </Row>
        {CHOICES.map((choice) => (
          <Row key={choice.value}>
            <Radio
              name={group}
              value={choice.value}
              checked={on === choice.on}
              disabled={busy || on === null}
              onChange={() => void change(choice.on)}
              className="flex-1 pl-10"
            >
              <span className="text-body font-medium">{choice.name}</span>
              <span className={rowDetailVariants({ mono: false })}>{choice.detail}</span>
            </Radio>
          </Row>
        ))}
      </fieldset>
    </SettingsSection>
  );
}
