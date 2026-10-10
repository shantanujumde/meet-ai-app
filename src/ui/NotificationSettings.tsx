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
 * * "Only for meetings with at least N people" is `detection.min_attendees`,
 *   from Rust's `MIN_ATTENDEES` to `MAX_ATTENDEES` (a value set by hand
 *   outside them is read as the nearest end, and shown as it is).
 * * "Ask to record when a call starts" is `detection.call_start` (TUR-143):
 *   a call app or a browser using the mic for 15 seconds, named in the
 *   prompt. The "Never detect" list under it is `detection.never_detect`
 *   ({@link NeverDetectList}).
 * * "Ask to stop when a call ends" is `detection.call_end` (TUR-144): when the
 *   call app hangs up, a 10-second countdown asks before stopping.
 * * "Stop after 10 min of silence" is `detection.stop_after_silence`
 *   (TUR-145). Sleep always stops a recording; it has no switch.
 *
 * A change saves the whole section through Rust's comment-keeping writer and
 * shows what was saved; the reminder and detection loops pick it up on their
 * next tick, with no restart. A failed save keeps the old values and says
 * why. A value in config.jsonc Rust could not use is named under the card
 * (TUR-155); the card still saves, since only the changed key is written.
 * When the OS is blocking meet-ai's notifications, a line says so and
 * opens the OS page that allows them. "Send a test reminder" shows the
 * reminder for a fake meeting, which never records.
 */

import {
  AppWindow,
  AudioLines,
  Bell,
  FlaskConical,
  Settings as Gear,
  Hourglass,
  PhoneIncoming,
  PhoneOff,
  Timer,
  Users,
} from "lucide-react";
import { useId, useState } from "react";
import { useIpcValue, useSavedSetting } from "@/hooks/useIpcValue";
import { MAX_ATTENDEES, MIN_ATTENDEES } from "@/ipc/bindings";
import {
  notificationSettings,
  openNotificationSettings,
  osNotificationsBlocked,
  type NotificationSettings as Settings,
  type SettingsSnapshot,
  sendTestReminder,
  setNotificationSettings,
} from "@/ipc/client";
import { toUiError } from "@/ipc/types";
import { osText } from "@/lib/osText";
import type { LucideIcon } from "./icons";
import { NeverDetectList } from "./NeverDetectList";
import { Button } from "./primitives";
import { Switch } from "./SettingSwitch";
import { SETTINGS_SELECT, SettingsRow, SettingsSection } from "./settings/SettingsSection";
import { useSnapshotLoad } from "./settings/snapshot";
import { ConfigProblemNote, useConfigProblem } from "./settings/useConfigProblem";
import { ErrorState } from "./states";

export const REMIND_LABEL = "Remind me before meetings";
export const LEAD_LABEL = "How early";
export const PROCESSES_LABEL = "Ask when a meeting app is running";
export const AUDIO_LABEL = "Ask when my mic and speakers are both in use";
export const ATTENDEES_LABEL = "Only for meetings with at least";
export const CALL_START_LABEL = "Ask to record when a call starts";
export const CALL_END_LABEL = "Ask to stop when a call ends";
export const SILENCE_LABEL = "Stop after 10 min of silence";

/** The lead times the card offers, in minutes. */
export const LEAD_MINUTES = [1, 2, 5, 10];

/** "Only for meetings with at least N people": Rust's one range, 1 to 10. */
export const ATTENDEE_COUNTS = Array.from(
  { length: MAX_ATTENDEES - MIN_ATTENDEES + 1 },
  (_, i) => i + MIN_ATTENDEES,
);

export function attendeesLabel(count: number): string {
  return `${count} ${count === 1 ? "person" : "people"}`;
}

/** `choices`, plus `saved` in order when it is not one of them. */
export function withSaved(choices: number[], saved: number | undefined): number[] {
  if (saved === undefined || choices.includes(saved)) return choices;
  return [...choices, saved].sort((a, b) => a - b);
}

export function leadLabel(minutes: number): string {
  if (minutes === 0) return "At the start";
  return `${minutes} ${minutes === 1 ? "minute" : "minutes"} before`;
}

const pick = (snapshot: SettingsSnapshot) => snapshot.notifications;

export function NotificationSettings() {
  const load = useSnapshotLoad(pick, notificationSettings);
  const {
    value: settings,
    busy,
    error,
    setError,
    change: saveAll,
  } = useSavedSetting(load, setNotificationSettings);
  // Not knowing is not the same as blocked: a failed read says nothing.
  const blocked = useIpcValue(osNotificationsBlocked).value ?? false;
  const [testNote, setTestNote] = useState<string | null>(null);
  const problem = useConfigProblem("detection", settings);

  const save = async (change: Partial<Settings>) => {
    if (settings === null) return;
    await saveAll({ ...settings, ...change });
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
  const leadChoices = withSaved(LEAD_MINUTES, settings?.remindBeforeMinutes);
  const attendeeChoices = withSaved(ATTENDEE_COUNTS, settings?.minAttendees);

  return (
    <SettingsSection
      title="Notifications"
      after={error ? <ErrorState error={error} /> : <ConfigProblemNote problem={problem} />}
    >
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
        options={attendeeChoices.map((count) => ({ value: count, label: attendeesLabel(count) }))}
        onChange={(minAttendees) => void save({ minAttendees })}
      />
      <SwitchRow
        icon={Hourglass}
        label={SILENCE_LABEL}
        detail="When no one has spoken for 10 minutes, asks, then stops in 10 seconds."
        on={settings?.stopAfterSilence ?? false}
        disabled={disabled}
        onChange={(stopAfterSilence) => void save({ stopAfterSilence })}
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
