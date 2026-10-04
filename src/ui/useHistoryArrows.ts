/**
 * Whether the title bar's back and forward arrows have anywhere to go
 * (TUR-102).
 *
 * The router keeps its place in the history as `window.history.state.idx`
 * (0 is the first entry this window made), so back is possible above 0.
 * Forward is possible while the user has gone back and not yet gone somewhere
 * new: a push or a replace drops everything ahead, a back or forward (a POP)
 * keeps it.
 */

import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigationType } from "react-router";

/** Where the router says this entry sits, or 0 outside a router-made entry. */
function historyIndex(): number {
  const state = window.history.state as { idx?: unknown } | null;
  return typeof state?.idx === "number" ? state.idx : 0;
}

/** The furthest entry still reachable, after a navigation of `type` to `index`. */
export function furthestAfter(type: "POP" | "PUSH" | "REPLACE", index: number, furthest: number) {
  return type === "POP" ? Math.max(furthest, index) : index;
}

export function useHistoryArrows(): { canBack: boolean; canForward: boolean } {
  const location = useLocation();
  const type = useNavigationType();
  const furthest = useRef(historyIndex());
  const [state, setState] = useState({ canBack: false, canForward: false });

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-read on every navigation; the key is the trigger.
  useEffect(() => {
    const index = historyIndex();
    furthest.current = furthestAfter(type, index, furthest.current);
    setState({ canBack: index > 0, canForward: index < furthest.current });
  }, [location.key, type]);

  return state;
}
