/**
 * The calendars Today could not read while others answered (TUR-174): an
 * expired Google sign-in next to the Calendar app's meetings. One line each,
 * above the day, so a partial day never reads as the whole day, and a way to
 * Settings → Calendars, where "Sign in again" lives.
 */

import { Settings as SettingsIcon } from "lucide-react";
import { useNavigate } from "react-router";
import type { UnreadableCalendar } from "@/ipc/client";
import { settingsPath } from "@/lib/routes";
import { Button, ButtonRow } from "../primitives";

/** The Settings link under the lines. */
export const OPEN_CALENDAR_SETTINGS = "Open Calendar settings";

/** One calendar's line. */
export function unreadableCopy(calendar: UnreadableCalendar): string {
  if (calendar.kind === "calendar-sign-in-expired") {
    return `${calendar.provider} needs you to sign in again, so its meetings are missing here.`;
  }
  return `${calendar.provider} could not be read, so its meetings are missing here: ${calendar.message}`;
}

export function UnreadableCalendars({ unreadable }: { unreadable: UnreadableCalendar[] }) {
  const navigate = useNavigate();
  if (unreadable.length === 0) return null;
  return (
    <div className="flex flex-col gap-2" role="alert">
      {unreadable.map((calendar) => (
        <p key={calendar.provider} className="m-0 text-footnote text-danger wrap-anywhere">
          {unreadableCopy(calendar)}
        </p>
      ))}
      <ButtonRow>
        <Button
          size="small"
          tone="quiet"
          icon={SettingsIcon}
          onClick={() => navigate(settingsPath("calendars"))}
        >
          {OPEN_CALENDAR_SETTINGS}
        </Button>
      </ButtonRow>
    </div>
  );
}
