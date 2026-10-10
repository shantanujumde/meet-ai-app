/**
 * The note a Settings card shows when a value in its section of config.jsonc
 * was not valid (TUR-155): Rust read that value as its default and kept the
 * rest, and the card still saves (only the keys the user changes are
 * written). One wording for every card.
 */

import { useEffect, useState } from "react";
import { type ConfigSection, configProblem } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { InlineError } from "../states";

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
 */
export function useConfigProblem(section: ConfigSection, reload?: unknown): UiError | null {
  const [problem, setProblem] = useState<UiError | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reload` only re-asks.
  useEffect(() => {
    let live = true;
    configProblem(section)
      .then((found) => {
        if (live) setProblem(found ? configProblemError(found) : null);
      })
      .catch(() => {
        if (live) setProblem(null);
      });
    return () => {
      live = false;
    };
  }, [section, reload]);
  return problem;
}

/** The note itself, or nothing. */
export function ConfigProblemNote({ problem }: { problem: UiError | null }) {
  return problem ? <InlineError error={problem} /> : null;
}
