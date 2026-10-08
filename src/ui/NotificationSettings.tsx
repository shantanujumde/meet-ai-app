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
 * * "Ask to record when a call starts" is `detection.call_start` (TUR-143):
 *   a call app or a browser using the mic for 15 seconds, named in the
 *   prompt. The "Never detect" list under it is `detection.never_detect`
 *   ({@link NeverDetectList}).
 * * "Ask to stop when a call ends" is `detection.call_end` (TUR-144): when the
 *   call app hangs up, a 10-second countdown asks before stopping.
 *
 * A change saves the whole section through Rust's comment-keeping writer and
 * shows what was saved; the reminder and detection loops pick it up on their
 * next tick, with no restart. A failed save keeps the old values and says
 * why. When the OS is blocking meet-ai's notifications, a line says so and
 * opens the OS page that allows them. "Send a test reminder" shows the
 * reminder for a fake meeting, which never records.
 */

import {
  AppWindow,
  AudioLines,
  Bell,
  FlaskConical,
  Settings as Gear,
  PhoneIncoming,
  PhoneOff,
  Timer,
  Users,
} from "lucide-react";
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
import { osText } from "@/lib/osText";
import type { LucideIcon } from "./icons";
import { NeverDetectList } from "./NeverDetectList";
import { Button } from "./primitives";
import { Switch } from "./SettingSwitch";
import { SETTINGS_SELECT, SettingsRow, SettingsSection } from "./settings/SettingsSection";
import { ErrorState } from "./states";

export const REMIND_LABEL = "Remind me before meetings";
export const LEAD_LABEL = "How early";
export const PROCESSES_LABEL = "Ask when a meeting app is running";
export const AUDIO_LABEL = "Ask when my mic and speakers are both in use";
export const ATTENDEES_LABEL = "Only for meetings with at least";
export const CALL_START_LABEL = "Ask to record when a call starts";
export const CALL_END_LABEL = "Ask to stop when a call ends";

/** The lead times the card offers, in minutes. */
export const LEAD_MINUTES = [1, 2, 5, 10];

/** "Only for meetings with at least N people": 1 to 10. */
export const ATTENDEE_COUNTS = Array.from({ length: 10 }, (_, i) => i + 1);

export function leadLabel(minutes: number): string {
  if (minutes === 0) return "At the start";
  return `${minutes} ${minutes === 1 ? "minute" : "minutes"} before`;
}

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
    <SettingsSection title="Notifications" after={error ? <ErrorState error={error} /> : null}>
      <SwitchRow
        icon={Bell}
        label={REMIND_LABEL}
        detail="Before each meeting on your calendar, with Join and Record."
        on={settings?.remind ?? false}
        disabled={disabled}
        onChange={(remind) => void save({ remind })}
      />
      <SelectRow
        icon={Timer}
        label={LEAD_LABEL}
        value={settings?.remindBeforeMinutes ?? 1}
        disabled={disabled || settings?.remind === false}
        options={leadChoices.map((minutes) => ({ value: minutes, label: leadLabel(minutes) }))}
        onChange={(remindBeforeMinutes) => void save({ remindBeforeMinutes })}
      />
      <SwitchRow
        icon={PhoneIncoming}
        label={CALL_START_LABEL}
        detail="When an app like WhatsApp or a browser has used the mic for 15 seconds."
        on={settings?.callStart ?? false}
        disabled={disabled}
        onChange={(callStart) => void save({ callStart })}
      />
      <NeverDetectList onError={setError} />
      <SwitchRow
        icon={AppWindow}
        label={PROCESSES_LABEL}
        detail="Zoom, Teams, Webex and the like, even without an invite."
        on={settings?.processes ?? false}
        disabled={disabled}
        onChange={(processes) => void save({ processes })}
      />
      <SwitchRow
        icon={AudioLines}
        label={AUDIO_LABEL}
        detail="Like a call in a browser tab."
        on={settings?.audioActivity ?? false}
        disabled={disabled}
        onChange={(audioActivity) => void save({ audioActivity })}
      />
      <SwitchRow
        icon={PhoneOff}
        label={CALL_END_LABEL}
        detail="When the call app hangs up, stops in 10 seconds unless you keep recording."
        on={settings?.callEnd ?? false}
        disabled={disabled}
        onChange={(callEnd) => void save({ callEnd })}
      />
      <SelectRow
        icon={Users}
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
        <SettingsRow
          icon={Gear}
          name="macOS is blocking meet-ai's notifications"
          detail="The prompt still waits in the meet-ai window."
          control={
            <Button
              size="small"
              onClick={() => void openNotificationSettings().catch((e) => setError(toUiError(e)))}
            >
              Open {osText("settings")}
            </Button>
          }
        />
      ) : null}
      <SettingsRow
        icon={FlaskConical}
        name="Test"
        detail={testNote ?? "Shows a reminder for a fake meeting. It never records."}
        control={
          <Button size="small" icon={Bell} onClick={() => void test()}>
            Send a test reminder
          </Button>
        }
      />
    </SettingsSection>
  );
}

function SwitchRow({
  icon,
  label,
  detail,
  on,
  disabled,
  onChange,
}: {
  icon: LucideIcon;
  label: string;
  detail: string;
  on: boolean;
  disabled: boolean;
  onChange: (on: boolean) => void;
}) {
  const id = useId();
  return (
    <SettingsRow
      icon={icon}
      name={<label htmlFor={id}>{label}</label>}
      detail={detail}
      control={<Switch id={id} on={on} disabled={disabled} onChange={onChange} />}
    />
  );
}

function SelectRow({
  icon,
  label,
  detail,
  value,
  options,
  disabled,
  onChange,
}: {
  icon: LucideIcon;
  label: string;
  detail?: string;
  value: number;
  options: { value: number; label: string }[];
  disabled: boolean;
  onChange: (value: number) => void;
}) {
  const id = useId();
  return (
    <SettingsRow
      icon={icon}
      name={<label htmlFor={id}>{label}</label>}
      detail={detail}
      control={
        <select
          id={id}
          className={SETTINGS_SELECT}
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
      }
    />
  );
}
