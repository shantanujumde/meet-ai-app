/**
 * Put a scroll container back at the top whenever the route changes (TUR-82).
 *
 * The shell's content pane is one element that every route renders into, so
 * its `scrollTop` outlives the route that set it: scroll Settings to the
 * bottom, open a meeting, and the meeting's title is off the top of the pane.
 * Each route is a new page, so it starts at the top.
 *
 * Keyed on the pathname, which carries the meeting id, so one meeting to
 * another resets too. A layout effect, so the pane is at the top before the
 * new route first paints, rather than flashing the old offset.
 *
 * It only ever touches the element it is given: a route's own inner scroller
 * (the live transcript's box) keeps its position.
 */

import { type RefObject, useLayoutEffect } from "react";
import { useLocation } from "react-router";

export function useResetScrollOnRouteChange(ref: RefObject<HTMLElement | null>): void {
  const { pathname } = useLocation();

  // biome-ignore lint/correctness/useExhaustiveDependencies: the pathname is the trigger, not a value the effect reads.
  useLayoutEffect(() => {
    const element = ref.current;
    if (element && element.scrollTop !== 0) element.scrollTop = 0;
  }, [pathname, ref]);
}
