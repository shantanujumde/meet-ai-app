/**
 * The record shortcut as the window should show it (TUR-169): its label
 * ("⌘⇧R", "Ctrl+Alt+R", from `osText.ts`) and whether pressing it reaches
 * meet-ai. Another app may own it, and then the window says it is
 * unavailable instead of advertising it.
 */

import { recordShortcutAvailable } from "@/ipc/shortcut";
import { shortcutLabel } from "@/lib/osText";
import { useIpcValue } from "./useIpcValue";

export type RecordShortcut = { label: string; available: boolean };

export function useRecordShortcut(): RecordShortcut {
  // Advertised until Rust says otherwise; `recordShortcutAvailable` does not reject.
  const available = useIpcValue(recordShortcutAvailable).value ?? true;
  return { label: shortcutLabel(), available };
}

/** The tooltip for the shortcut that another app owns. */
export function unavailableText(label: string): string {
  return `${label} is unavailable: another app is using it. Use the Record button instead.`;
}
