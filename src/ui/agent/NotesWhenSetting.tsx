/**
 * Settings → When notes run (TUR-101): `agent.auto_run` as two choices.
 * "Automatically after the call" (the default) starts the notes run once the
 * transcript is final; "Only when I click" starts nothing, and the meeting
 * shows "Make notes now" instead. Settings only, not onboarding.
 */

import { Clock } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { notesAutoRun, saveNotesAutoRun } from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { IconSquare } from "@/ui/icons";
import { Row, rowDetailVariants } from "@/ui/primitives";
import { Radio } from "@/ui/Radio";
import { SettingsSection } from "@/ui/settings/SettingsSection";
import { ErrorState } from "@/ui/states";

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

export function NotesWhenSetting() {
  const group = useId();
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);

  useEffect(() => {
    let live = true;
    notesAutoRun()
      .then((saved) => {
        if (live) setOn(saved);
      })
      .catch((thrown: unknown) => {
        if (live) setError(toUiError(thrown));
      });
    return () => {
      live = false;
    };
  }, []);

  const change = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setOn(await saveNotesAutoRun(next));
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  };

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
