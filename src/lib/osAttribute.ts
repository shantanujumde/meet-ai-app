/**
 * Marks `<html>` with the OS the window runs on (TUR-57), so CSS can drop the
 * macOS-only chrome (the traffic-light inset, the see-through sidebar) on
 * Windows and Linux without touching the macOS look.
 */

import { currentOs, type Os } from "./osText";

export function applyOsAttribute(
  root: HTMLElement = document.documentElement,
  os: Os = currentOs(),
): void {
  root.dataset.os = os;
}
