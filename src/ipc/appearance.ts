/**
 * Light / Dark / System and the glass switch (TUR-102): the `appearance`
 * section of `config.jsonc`.
 *
 * Re-exported from `./client`; import from there.
 */

import { DEFAULT_APPEARANCE } from "@/lib/constants";
import {
  type meet_ai_lib_config_appearance_section_AppearanceConfig as Appearance,
  commands,
  type meet_ai_lib_config_appearance_section_Theme as ThemeChoice,
} from "./bindings";
import { call, hasBackend } from "./client";

export type { Appearance, ThemeChoice };

/** The theme and glass switch as saved. */
export async function appearanceSettings(): Promise<Appearance> {
  if (!hasBackend()) return DEFAULT_APPEARANCE;
  return call(() => commands.appearanceSettings());
}

/** Save both. Resolves to what was saved. */
export function setAppearance(appearance: Appearance): Promise<Appearance> {
  return call(() => commands.setAppearance(appearance));
}
