/**
 * The one way this window puts text on the clipboard.
 *
 * Inside the Mac app it goes through Tauri's clipboard plugin, which works
 * whether or not the webview has focus. Outside it — `pnpm dev` in a browser —
 * the plugin has nothing to talk to, so it falls back to the browser's own
 * clipboard.
 *
 * Either can refuse (no permission, no focus). That is reported by throwing,
 * and each caller decides what refusal means for it: "Copy details" shrugs,
 * since the text is already on screen; a copied prompt shows the text so it
 * can be copied by hand.
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { hasBackend } from "@/ipc/client";

export async function copyText(text: string): Promise<void> {
  if (hasBackend()) {
    await writeText(text);
  } else {
    await navigator.clipboard.writeText(text);
  }
}
