/**
 * Closing and quitting (TUR-76): the `app` settings, and the "Stop recording
 * and quit?" question Rust asks while it holds a quit.
 *
 * Re-exported from `./client`; import from there.
 */

import { DEFAULT_APP_SETTINGS, DEFAULT_MENU_BAR_COUNTDOWN } from "@/lib/constants";
import {
  type meet_ai_lib_lifecycle_AppSettings as AppSettings,
  commands,
  NAVIGATE_EVENT,
  type meet_ai_lib_lifecycle_NavigateTo as NavigateTo,
  QUIT_CONFIRM_EVENT,
} from "./bindings";
import { call, hasBackend, subscribe } from "./client";

export type { AppSettings, NavigateTo };
export { NAVIGATE_EVENT, QUIT_CONFIRM_EVENT };

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

/** "Show next meeting in the menu bar" (TUR-77): `app.menu_bar_countdown`. */
export async function menuBarCountdown(): Promise<boolean> {
  if (!hasBackend()) return DEFAULT_MENU_BAR_COUNTDOWN;
  return call(() => commands.menuBarCountdown());
}

/** Save "Show next meeting in the menu bar". Resolves to what was saved. */
export function setMenuBarCountdown(show: boolean): Promise<boolean> {
  return call(() => commands.setMenuBarCountdown(show));
}

/**
 * Rust wants the window on a screen (TUR-77): the menu bar's "Open brief"
 * and "Calendar not connected".
 */
export function onNavigate(handler: (to: NavigateTo) => void): () => void {
  return subscribe<NavigateTo>(NAVIGATE_EVENT, handler);
}
