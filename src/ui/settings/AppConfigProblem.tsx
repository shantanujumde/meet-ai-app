/**
 * The invalid-config note for the Menu bar section (TUR-155, TUR-170): the
 * `app` section of config.jsonc, behind the Dock and menu-bar switches.
 *
 * The switches save themselves, one key each, so they tell this note through
 * {@link appSettingSaved}; a save that wrote over the bad value clears it.
 */

import { create } from "zustand";
import { ConfigProblemNote, useConfigProblem } from "./useConfigProblem";

const useAppSaves = create<{ saves: number }>(() => ({ saves: 0 }));

/** A switch in the `app` section saved: the note asks again. */
export function appSettingSaved() {
  useAppSaves.setState(({ saves }) => ({ saves: saves + 1 }));
}

export function AppConfigProblem() {
  const saves = useAppSaves((state) => state.saves);
  return <ConfigProblemNote problem={useConfigProblem("app", saves)} />;
}
