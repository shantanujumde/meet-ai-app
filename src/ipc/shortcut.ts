/**
 * Whether the record shortcut is meet-ai's (TUR-169). Registering it fails
 * when another app already owns it; then pressing it does nothing here, and
 * the window says so instead of advertising it. Its own file so `client.ts`
 * stays under the size limit.
 */

import { commands } from "./bindings";
import { hasBackend } from "./client";

/** `false` only when Rust tried to register the shortcut and could not. */
export async function recordShortcutAvailable(): Promise<boolean> {
  if (!hasBackend()) return true;
  try {
    return await commands.recordShortcutAvailable();
  } catch {
    // Nothing to ask: keep advertising it, as before TUR-169.
    return true;
  }
}
