/**
 * Settings → Notifications and the reminder banner's Join and Record
 * (TUR-78).
 *
 * Re-exported from `./client`; import from there.
 */

import {
  commands,
  type meet_ai_lib_detection_settings_NotificationSettings as NotificationSettings,
} from "./bindings";
import { call, hasBackend } from "./client";

export type { NotificationSettings };

/** SPEC §3.5's `detection` defaults, plus TUR-78's one-minute lead time. */
export const DEFAULT_NOTIFICATION_SETTINGS: NotificationSettings = {
  remind: true,
  remindBeforeMinutes: 1,
  processes: true,
  audioActivity: true,
  minAttendees: 2,
  callStart: true,
};

/** The `detection` section of `config.jsonc`, as the card shows it. */
export async function notificationSettings(): Promise<NotificationSettings> {
  if (!hasBackend()) return DEFAULT_NOTIFICATION_SETTINGS;
  return call(() => commands.notificationSettings());
}

/** Save the card into `config.jsonc`; applies without a restart. Resolves to what was saved. */
export function setNotificationSettings(
  settings: NotificationSettings,
): Promise<NotificationSettings> {
  return call(() => commands.setNotificationSettings(settings));
}

/**
 * The "Never detect" list (TUR-143): the apps a prompt's "Never for" added,
 * by name ("WhatsApp"), or by id when written into `config.jsonc` by hand.
 */
export async function neverDetectApps(): Promise<string[]> {
  if (!hasBackend()) return [];
  return call(() => commands.neverDetectApps());
}

/** Save the "Never detect" list; applies without a restart. Resolves to what was saved. */
export function setNeverDetectApps(apps: string[]): Promise<string[]> {
  return call(() => commands.setNeverDetectApps(apps));
}

/** Whether the OS is blocking meet-ai's notifications. */
export async function osNotificationsBlocked(): Promise<boolean> {
  if (!hasBackend()) return false;
  return call(() => commands.osNotificationsBlocked());
}

/** Open the OS page where meet-ai's notifications are allowed. */
export async function openNotificationSettings(): Promise<void> {
  await call(() => commands.openNotificationSettings());
}

/** "Send a test reminder". Resolves to `false` when a recording kept it from showing. */
export function sendTestReminder(): Promise<boolean> {
  return call(() => commands.sendTestReminder());
}

/** The banner's **Join**: open the reminded meeting's link. */
export async function joinRemindedMeeting(eventId: string): Promise<void> {
  await call(() => commands.joinRemindedMeeting(eventId));
}

/**
 * The banner's **Record**, or with `join` **Join and record**, for a
 * reminded meeting: the recording is named after it. The new state arrives
 * on the recording event like any start.
 */
export async function recordRemindedMeeting(eventId: string, join: boolean): Promise<void> {
  await call(() => commands.recordRemindedMeeting(eventId, join));
}
