/**
 * The recording overlay (TUR-146): its Settings switch
 * (`audio.show_recording_overlay`, on by default) and the one thing the
 * overlay window asks of Rust beyond the recording commands, bringing the
 * main window forward. Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call, hasBackend } from "./client";

/** On unless `config.jsonc` turns it off. */
export const DEFAULT_SHOW_RECORDING_OVERLAY = true;

/** The setting as saved. */
export async function showRecordingOverlay(): Promise<boolean> {
  if (!hasBackend()) return DEFAULT_SHOW_RECORDING_OVERLAY;
  return call(() => commands.showRecordingOverlay());
}

/** Save the setting. Resolves to what was saved; the overlay follows at once. */
export function setShowRecordingOverlay(on: boolean): Promise<boolean> {
  return call(() => commands.setShowRecordingOverlay(on));
}

/** The overlay's text was clicked: bring the main window to the front. */
export function overlayShowMain(): Promise<void> {
  return call(() => commands.overlayShowMain());
}
