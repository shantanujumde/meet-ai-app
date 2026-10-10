/**
 * The small "Hook failed" note in the meeting view (TUR-63). A user hook that
 * failed never stops the meeting; this only says so, with what went wrong.
 * The full output is in the app log. The failures are kept from app start
 * ({@link useMeetingHookFailures}), so one that happened while this meeting
 * was not open still shows (TUR-170).
 */

import { useMeetingHookFailures } from "@/state/hookFailures";

export function HookFailedNote({ meetingId }: { meetingId: string }) {
  const failures = useMeetingHookFailures(meetingId);

  if (failures.length === 0) return null;
  return (
    <div role="status">
      {failures.map((failed) => (
        <p key={failed.hook} className="text-footnote text-danger wrap-anywhere">
          Hook failed: {failed.hook} ({failed.message}). Details are in the log.
        </p>
      ))}
    </div>
  );
}
