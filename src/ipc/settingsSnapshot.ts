/**
 * Every saved setting the Settings screen shows, in one read (TUR-171).
 *
 * Settings used to ask a dozen commands on every visit, each reading the
 * meetings root and `config.jsonc` again. `settings_snapshot` reads them
 * once; the cards still save through their own commands.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands, type meet_ai_lib_settings_snapshot_SettingsSnapshot } from "./bindings";
import { hasBackend, narrow } from "./client";
import type { TrackerSettings, UiError } from "./types";

/**
 * The generated shape, with Rust's plain strings narrowed as `./types` does
 * for the commands each field stands in for.
 */
export type SettingsSnapshot = Omit<
  meet_ai_lib_settings_snapshot_SettingsSnapshot,
  "agentError" | "tracker" | "trackerError"
> & {
  agentError: UiError | null;
  tracker: TrackerSettings | null;
  trackerError: UiError | null;
};

/**
 * The snapshot, or null with no Rust behind the window: then each card asks
 * its own command, which knows its no-backend answer.
 */
export async function settingsSnapshot(): Promise<SettingsSnapshot | null> {
  if (!hasBackend()) return null;
  return narrow(() => commands.settingsSnapshot());
}
