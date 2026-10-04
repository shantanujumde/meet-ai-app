/**
 * The words that change with the operating system (TUR-51).
 *
 * One place for "System Settings" vs "Settings", "Finder" vs "File Explorer",
 * so copy elsewhere names the thing the user actually sees. The OS comes from
 * `@tauri-apps/plugin-os`; without the app behind the window (a plain browser,
 * a component test) it is macOS, the platform the copy was first written for.
 */

import { platform } from "@tauri-apps/plugin-os";

export type Os = "macos" | "windows" | "linux";

/** The OS this window runs on. Anything that is not Windows or Linux is macOS. */
export function currentOs(): Os {
  try {
    const name = platform();
    if (name === "windows" || name === "linux") return name;
  } catch {
    // No plugin behind the window.
  }
  return "macos";
}

const WORDS = {
  settings: { macos: "System Settings", windows: "Settings", linux: "Settings" },
  fileManager: { macos: "Finder", windows: "File Explorer", linux: "file manager" },
  terminal: { macos: "Terminal", windows: "PowerShell", linux: "Terminal" },
} as const satisfies Record<string, Record<Os, string>>;

export type OsWord = keyof typeof WORDS;

/** The OS's own name for `word`. */
export function osText(word: OsWord, os: Os = currentOs()): string {
  return WORDS[word][os];
}
