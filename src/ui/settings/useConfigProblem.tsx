/**
 * The note a Settings card shows when a value in its section of config.jsonc
 * was not valid (TUR-155): Rust read that value as its default and kept the
 * rest, and the card still saves (only the keys the user changes are
 * written). One wording for every card.
 */

import { useEffect, useRef, useState } from "react";
import { type ConfigSection, configProblem, type SettingsSnapshot } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { InlineError } from "../states";
import { fromSnapshot, useSettingsSnapshot } from "./snapshot";

/** The sentence in front of Rust's own words for what was not valid. */
export const CONFIG_PROBLEM_LEAD =
  "This setting in config.jsonc was not valid and is shown as the default:";

/** `problem` (Rust's words) as the note's error, lead sentence first. */
export function configProblemError(problem: string): UiError {
  return { domain: "app", kind: "invalid-config", message: `${CONFIG_PROBLEM_LEAD} ${problem}` };
}

/**
 * What in `section` was not valid, read once when the card mounts and again
 * whenever `reload` changes (pass the saved values, so a save that fixed the
 * value clears the note). Null while loading, when all is valid, and when it
 * cannot be asked: the card's own errors cover a broken backend.
 *
 * Inside Settings the first answer is the visit's snapshot (TUR-171): the
 * first saved values the card shows came from that same read, so only a
 * later change (a save) asks again.
 */
export function useConfigProblem(section: ConfigSection, reload?: unknown): UiError | null {
  const [problem, setProblem] = useState<UiError | null>(null);
  const snapshot = useSettingsSnapshot();
  // How many saved values the card has shown: the first is the snapshot's.
  const shown = useRef({ last: undefined as unknown, count: 0 });
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reload` only re-asks.
  useEffect(() => {
    let live = true;
    if (reload !== shown.current.last) {
      shown.current.last = reload;
      if (reload !== null && reload !== undefined) shown.current.count += 1;
    }
    const ask =
      snapshot !== null && shown.current.count <= 1
        ? fromSnapshot(
            snapshot,
            (answer) => problemIn(answer, section),
            () => configProblem(section),
          )
        : configProblem(section);
    ask
      .then((found) => {
        if (live) setProblem(found ? configProblemError(found) : null);
      })
      .catch(() => {
        if (live) setProblem(null);
      });
    return () => {
      live = false;
    };
  }, [section, reload, snapshot]);
  return problem;
}

/** `section`'s problem as the snapshot read it. */
function problemIn(snapshot: SettingsSnapshot, section: ConfigSection): string | null {
  switch (section) {
    case "app":
      return snapshot.appProblem;
    case "appearance":
      return snapshot.appearanceProblem;
    case "detection":
      return snapshot.detectionProblem;
  }
}

/** The note itself, or nothing. */
export function ConfigProblemNote({ problem }: { problem: UiError | null }) {
  return problem ? <InlineError error={problem} /> : null;
}
