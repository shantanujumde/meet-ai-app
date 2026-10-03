/**
 * Closing and quitting (TUR-76): the `app` settings, and the "Stop recording
 * and quit?" question Rust asks while it holds a quit.
 *
 * Re-exported from `./client`; import from there.
 */

import { DEFAULT_APP_SETTINGS } from "@/lib/constants";
import {
  type meet_ai_lib_lifecycle_AppSettings as AppSettings,
  commands,
  QUIT_CONFIRM_EVENT,
} from "./bindings";
import { call, hasBackend, subscribe } from "./client";

export type { AppSettings };
export { QUIT_CONFIRM_EVENT };

/** How meet-ai behaves as an app: the `app` section of `config.jsonc`. */
export async function appSettings(): Promise<AppSettings> {
  if (!hasBackend()) return DEFAULT_APP_SETTINGS;
  return call(() => commands.appSettings());
}

/** "Show in Dock when the window is closed" (macOS). Resolves to what was saved. */
export function setShowInDockWhenClosed(show: boolean): Promise<AppSettings> {
  return call(() => commands.setShowInDockWhenClosed(show));
}

/**
 * ⌘Q or the menu-bar Quit came in while recording, and Rust is holding it
 * until the user answers "Stop recording and quit?".
 */
export function onQuitConfirm(handler: () => void): () => void {
  return subscribe<null>(QUIT_CONFIRM_EVENT, () => handler());
}

/** "Stop and quit": Rust stops the recording through its normal stop path and quits. */
export async function confirmQuit(): Promise<void> {
  await call(() => commands.confirmQuit());
}
