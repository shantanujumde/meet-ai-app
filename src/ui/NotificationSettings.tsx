/**
 * Settings → Notifications (TUR-78): when meet-ai asks "Record this
 * meeting?", and how early it reminds.
 *
 * Every control is one key of the `detection` section of `config.jsonc`:
 *
 * * "Remind me before meetings" is `detection.calendar`, with the lead time
 *   `detection.remind_before_minutes` (1, 2, 5 or 10 minutes; a value set by
 *   hand, such as 0 for "at the start", is shown as it is).
 * * "Ask when a meeting app is running" is `detection.processes`.
 * * "Ask when my mic and speakers are both in use" is
 *   `detection.audio_activity`.
 * * "Only for meetings with at least N people" is `detection.min_attendees`.
 *
 * A change saves the whole section through Rust's comment-keeping writer and
 * shows what was saved; the reminder and detection loops pick it up on their
 * next tick, with no restart. A failed save keeps the old values and says
 * why. When the OS is blocking meet-ai's notifications, a line says so and
 * opens the OS page that allows them. "Send a test reminder" shows the
 * reminder for a fake meeting, which never records.
 */

import { useEffect, useId, useState } from "react";
import {
  notificationSettings,
  openNotificationSettings,
  osNotificationsBlocked,
  type NotificationSettings as Settings,
  sendTestReminder,
  setNotificationSettings,
} from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { Button, Card, Row, RowLabel } from "./primitives";
import { Switch } from "./SettingSwitch";
import { ErrorState } from "./states";

export const REMIND_LABEL = "Remind me before meetings";
export const LEAD_LABEL = "How early";
export const PROCESSES_LABEL = "Ask when a meeting app is running";
export const AUDIO_LABEL = "Ask when my mic and speakers are both in use";
export const ATTENDEES_LABEL = "Only for meetings with at least";

/** The lead times the card offers, in minutes. */
export const LEAD_MINUTES = [1, 2, 5, 10];

/** "Only for meetings with at least N people": 1 to 10. */
export const ATTENDEE_COUNTS = Array.from({ length: 10 }, (_, i) => i + 1);

export function leadLabel(minutes: number): string {
  if (minutes === 0) return "At the start";
  return `${minutes} ${minutes === 1 ? "minute" : "minutes"} before`;
}

const SELECT =
  "rounded-control border-[0.5px] border-separator bg-glass-sunken px-3 py-2 text-footnote text-fg-primary disabled:cursor-not-allowed disabled:opacity-40";

export function NotificationSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [blocked, setBlocked] = useState(false);
  const [testNote, setTestNote] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    notificationSettings()
      .then((saved) => {
        if (live) setSettings(saved);
      })
      .catch((thrown: unknown) => {
        if (live) setError(toUiError(thrown));
      });
    osNotificationsBlocked()
      .then((answer) => {
        if (live) setBlocked(answer);
      })
      // Not knowing is not the same as blocked: say nothing.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const save = async (change: Partial<Settings>) => {
    if (settings === null) return;
    setBusy(true);
    setError(null);
    try {
      setSettings(await setNotificationSettings({ ...settings, ...change }));
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  };

  const test = async () => {
    setTestNote(null);
    try {
      const sent = await sendTestReminder();
      setTestNote(sent ? "Sent." : "Not while recording.");
    } catch (thrown) {
      setError(toUiError(thrown));
    }
  };

  const disabled = busy || settings === null;
  const leadChoices =
    settings && !LEAD_MINUTES.includes(settings.remindBeforeMinutes)
      ? [...LEAD_MINUTES, settings.remindBeforeMinutes].sort((a, b) => a - b)
      : LEAD_MINUTES;

  return (
    <section className="section" aria-labelledby="notifications-heading">
      <h2 className="section__title" id="notifications-heading">
        Notifications
      </h2>
      <Card flush>
        <SwitchRow
          label={REMIND_LABEL}
          detail="Before each meeting on your calendar, with Join and Record."
          on={settings?.remind ?? false}
          disabled={disabled}
          onChange={(remind) => void save({ remind })}
        />
        <SelectRow
          label={LEAD_LABEL}
          value={settings?.remindBeforeMinutes ?? 1}
          disabled={disabled || settings?.remind === false}
          options={leadChoices.map((minutes) => ({ value: minutes, label: leadLabel(minutes) }))}
          onChange={(remindBeforeMinutes) => void save({ remindBeforeMinutes })}
        />
        <SwitchRow
          label={PROCESSES_LABEL}
          detail="Zoom, Teams, Webex and the like, even without an invite."
          on={settings?.processes ?? false}
          disabled={disabled}
          onChange={(processes) => void save({ processes })}
        />
        <SwitchRow
          label={AUDIO_LABEL}
          detail="Like a call in a browser tab."
          on={settings?.audioActivity ?? false}
          disabled={disabled}
          onChange={(audioActivity) => void save({ audioActivity })}
        />
        <SelectRow
          label={ATTENDEES_LABEL}
          detail="Calendar events with fewer people invited never remind."
          value={settings?.minAttendees ?? 2}
          disabled={disabled}
          options={ATTENDEE_COUNTS.map((count) => ({
            value: count,
            label: `${count} ${count === 1 ? "person" : "people"}`,
          }))}
          onChange={(minAttendees) => void save({ minAttendees })}
        />
        {blocked ? (
          <Row>
            <RowLabel
              name="macOS is blocking meet-ai's notifications"
              detail="The prompt still waits in the meet-ai window."
              mono={false}
            />
            <Button
              size="small"
              onClick={() => void openNotificationSettings().catch((e) => setError(toUiError(e)))}
            >
              Open System Settings
            </Button>
          </Row>
        ) : null}
        <Row>
          <RowLabel
            name="Test"
            detail={testNote ?? "Shows a reminder for a fake meeting. It never records."}
            mono={false}
          />
          <Button size="small" onClick={() => void test()}>
            Send a test reminder
          </Button>
        </Row>
      </Card>
      {error ? <ErrorState error={error} /> : null}
    </section>
  );
}

function SwitchRow({
  label,
  detail,
  on,
  disabled,
  onChange,
}: {
  label: string;
  detail: string;
  on: boolean;
  disabled: boolean;
  onChange: (on: boolean) => void;
}) {
  const id = useId();
  return (
    <Row>
      <RowLabel name={<label htmlFor={id}>{label}</label>} detail={detail} mono={false} />
      <Switch id={id} on={on} disabled={disabled} onChange={onChange} />
    </Row>
  );
}

function SelectRow({
  label,
  detail,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  detail?: string;
  value: number;
  options: { value: number; label: string }[];
  disabled: boolean;
  onChange: (value: number) => void;
}) {
  const id = useId();
  return (
    <Row>
      <RowLabel name={<label htmlFor={id}>{label}</label>} detail={detail} mono={false} />
      <select
        id={id}
        className={SELECT}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </Row>
  );
}
