/**
 * The brief "Copied" state every copy button shows (TUR-178).
 *
 * `copy(text)` puts `text` on the clipboard and reports whether it got there;
 * on success `copied` is true for `COPIED_RESET_MS`. A second copy restarts
 * that time rather than being cut short by the first one's timer, and the
 * timer never outlives the component. A refused clipboard leaves `copied`
 * false: each caller decides what to show instead.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { copyText } from "@/lib/clipboard";
import { COPIED_RESET_MS } from "@/lib/constants";

export function useCopied(): {
  copied: boolean;
  /** Resolves true once `text` is on the clipboard, false if it was refused. */
  copy: (text: string) => Promise<boolean>;
  /** Drop a "Copied" still on screen, for a new attempt that is not a copy yet. */
  reset: () => void;
} {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const reset = useCallback(() => {
    window.clearTimeout(timer.current);
    setCopied(false);
  }, []);

  const copy = useCallback(
    async (text: string) => {
      reset();
      try {
        await copyText(text);
      } catch {
        return false;
      }
      setCopied(true);
      timer.current = window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
      return true;
    },
    [reset],
  );

  return { copied, copy, reset };
}
