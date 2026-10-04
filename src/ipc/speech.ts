/**
 * The Settings speech engine picker (TUR-75): the saved engine and whisper
 * model, which choices this Mac can run (with Rust's reasons when not), and
 * saving a pick.
 *
 * Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call, hasBackend } from "./client";
import type { EngineChoice, EngineChoices, ModelCredit } from "./types";

/**
 * The saved choice, what "Automatic" lands on here, and which choices this
 * Mac can run, with the reason when not. Runs the ~160 ms probe, so never
 * await it before painting.
 */
export function engineChoices(): Promise<EngineChoices> {
  return call(() => commands.engineChoices());
}

/**
 * Save `transcription.engine` and `transcription.model` into config.jsonc,
 * keeping the rest of the file. Used from the next recording on. Resolves to
 * the picker as saved.
 */
export function setTranscription(engine: EngineChoice, model: string): Promise<EngineChoices> {
  return call(() => commands.setTranscription(engine, model));
}

/**
 * The model licences to credit in Settings, About (TUR-62: Parakeet's
 * CC-BY-4.0). Constant data from Rust; none without the app behind the window.
 */
export async function modelCredits(): Promise<ModelCredit[]> {
  if (!hasBackend()) return [];
  return call(() => commands.modelCredits());
}
