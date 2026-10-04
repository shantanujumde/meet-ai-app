/**
 * Puts the saved appearance on `<html>` (TUR-102): `data-theme="light"` or
 * `"dark"`, and `data-glass="off"` when the see-through look is switched off.
 * tokens.css keys every colour off those two attributes.
 *
 * `"system"` resolves through `prefers-color-scheme` and follows it while
 * the app runs. The webview always gets a resolved `light` or `dark`, so the
 * tokens have one dark block instead of one per way of asking for dark.
 */

import type { Appearance, ThemeChoice } from "@/ipc/client";

export type ResolvedTheme = "light" | "dark";

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** The colours `choice` means right now, given whether the OS is dark. */
export function resolveTheme(choice: ThemeChoice, systemDark: boolean): ResolvedTheme {
  if (choice === "system") return systemDark ? "dark" : "light";
  return choice;
}

/** Whether the OS asks for dark. False where there is no `matchMedia` (a test). */
export function systemIsDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(DARK_QUERY).matches;
}

/** Set `data-theme` and `data-glass` on `root` for `appearance`. */
export function applyAppearance(
  appearance: Appearance,
  root: HTMLElement = document.documentElement,
  systemDark: boolean = systemIsDark(),
): void {
  root.dataset.theme = resolveTheme(appearance.theme, systemDark);
  if (appearance.glass) delete root.dataset.glass;
  else root.dataset.glass = "off";
}

/**
 * Call `onChange` whenever the OS switches between light and dark. Returns
 * the unsubscribe. A no-op without `matchMedia`.
 */
export function watchSystemTheme(onChange: () => void): () => void {
  if (typeof window.matchMedia !== "function") return () => {};
  const query = window.matchMedia(DARK_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}
