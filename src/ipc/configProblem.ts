/**
 * "This setting in config.jsonc was not valid" (TUR-155): what in one
 * section of config.jsonc Rust read as its default, for the Settings card
 * that shows that section.
 *
 * Re-exported from `./client`; import from there.
 */

import {
  type meet_ai_lib_config_problem_ConfigSection as ConfigSection,
  commands,
} from "./bindings";
import { call, hasBackend } from "./client";

export type { ConfigSection };

/**
 * Rust's words for each bad value in `section` (its `app/invalid-config`
 * message), or null when all of it was valid. Null with no Rust behind the
 * window.
 */
export async function configProblem(section: ConfigSection): Promise<string | null> {
  if (!hasBackend()) return null;
  const found = await call(() => commands.configProblem(section));
  return found?.message ?? null;
}
