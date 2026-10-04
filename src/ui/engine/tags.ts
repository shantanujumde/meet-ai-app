/**
 * Fixed words for the speech card: the label on each model tag chip, and the
 * engine that actually runs for a saved choice.
 *
 * The facts themselves (name, good-for line, which tags) come from the Rust
 * catalogue (`crates/stt/src/model.rs`); only the chip labels live here, so
 * every row spells a tag the same way.
 */

import type { EngineChoices, ModelTag } from "@/ipc/types";

export const TAG_LABELS: Record<ModelTag, string> = {
  fast: "Fast",
  light: "Light",
  "most-accurate": "Most accurate",
  multilingual: "Multilingual",
  "english-only": "English only",
  slower: "Slower",
};

/**
 * The engine a recording would open with this choice: the forced one, or what
 * "Automatic" lands on (null when nothing is ready).
 */
export function engineInUse(
  choices: EngineChoices,
): "apple-speech" | "whisper" | "parakeet" | null {
  if (choices.engine === "auto") return choices.auto;
  return choices.engine;
}

/**
 * Locale ids as words, "en-US" → "English (United States)". Falls back to the
 * id itself where the browser has no name for it.
 */
export function languageNames(locales: string[]): string[] {
  let names: Intl.DisplayNames | null = null;
  try {
    names = new Intl.DisplayNames(undefined, { type: "language" });
  } catch {
    names = null;
  }
  return locales.map((locale) => {
    try {
      return names?.of(locale) ?? locale;
    } catch {
      return locale;
    }
  });
}
