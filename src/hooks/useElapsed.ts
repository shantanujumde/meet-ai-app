/**
 * A timer that ticks in the window rather than over IPC.
 *
 * Rust sends the start instant once; sending a tick per second would be an IPC
 * message per second for the entire length of a meeting, to render something
 * the webview can work out itself.
 */

import { useEffect, useState } from "react";

export function useElapsed(startedAtMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (startedAtMs === null) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [startedAtMs]);

  return startedAtMs === null ? 0 : Math.max(0, now - startedAtMs);
}
