/**
 * The small "Hook failed" note in the meeting view (TUR-63). A user hook that
 * failed never stops the meeting; this only says so, with what went wrong.
 * The full output is in the app log.
 */

import { useEffect, useState } from "react";
import { type HookFailed, onHookFailed } from "@/ipc/client";

export function HookFailedNote({ meetingId }: { meetingId: string }) {
  const [failures, setFailures] = useState<HookFailed[]>([]);

  useEffect(() => {
    setFailures([]);
    return onHookFailed((failed) => {
      if (failed.meetingId !== meetingId) return;
      setFailures((shown) => [...shown.filter((f) => f.hook !== failed.hook), failed]);
    });
  }, [meetingId]);

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
