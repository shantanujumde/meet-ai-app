/**
 * The words that change with the operating system (TUR-51).
 *
 * One place for "System Settings" vs "Settings", "Finder" vs "File Explorer",
 * so copy elsewhere names the thing the user actually sees. The OS comes from
 * `@tauri-apps/plugin-os`; without the app behind the window (a plain browser,
 * a component test) it is macOS, the platform the copy was first written for.
 */

import { platform } from "@tauri-apps/plugin-os";
import { RECORD_SHORTCUT_MAC, RECORD_SHORTCUT_OTHER } from "@/ipc/bindings";

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
  thisComputer: { macos: "this Mac", windows: "this PC", linux: "this computer" },
  appLauncher: { macos: "the Dock", windows: "the Start menu", linux: "the app menu" },
  trash: { macos: "Trash", windows: "Recycle Bin", linux: "Trash" },
} as const satisfies Record<string, Record<Os, string>>;

export type OsWord = keyof typeof WORDS;

/** The OS's own name for `word`. */
export function osText(word: OsWord, os: Os = currentOs()): string {
  return WORDS[word][os];
}

// Adapted from github.com/cjpais/Handy/src/lib/utils/keyboard.ts @ 73ab851c2b6242283759a4c101b60f0ece132f08 (MIT)
// (OS-aware modifier names: Command/Option on macOS, Ctrl/Alt elsewhere.)
const MAC_KEYS: Record<string, string> = {
  cmdorctrl: "⌘",
  cmd: "⌘",
  command: "⌘",
  ctrl: "⌃",
  control: "⌃",
  alt: "⌥",
  option: "⌥",
  shift: "⇧",
};
const OTHER_KEYS: Record<string, string> = {
  cmdorctrl: "Ctrl",
  ctrl: "Ctrl",
  control: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  super: "Super",
};

/**
 * The record shortcut as this OS writes it (TUR-58): "⌘⇧R" on macOS,
 * "Ctrl+Alt+R" on Windows and Linux. The value itself is Rust's
 * (`src-tauri/src/shortcut.rs`), through `bindings.ts`.
 */
export function shortcutLabel(os: Os = currentOs()): string {
  const accelerator = os === "macos" ? RECORD_SHORTCUT_MAC : RECORD_SHORTCUT_OTHER;
  const keys = os === "macos" ? MAC_KEYS : OTHER_KEYS;
  const parts = accelerator.split("+").map((key) => keys[key.toLowerCase()] ?? key.toUpperCase());
  return parts.join(os === "macos" ? "" : "+");
}
