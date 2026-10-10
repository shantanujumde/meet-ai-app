/**
 * The prompt card window's IPC (TUR-59, TUR-108, TUR-147): its card, its
 * answers, and the event that brings a new card. Its own file so
 * `client.ts` stays under the size limit; same rules as there: without a
 * backend, reads return nothing and writes reject with `no-backend`.
 */

import {
  commands,
  type meet_ai_lib_detection_popup_Card,
  type meet_ai_lib_detection_popup_PopupAnswer,
  type meet_ai_lib_detection_popup_PopupPrompt,
  PROMPT_POPUP_EVENT,
} from "./bindings";
import { call, hasBackend, subscribe } from "./client";

/** What the popup shows: its card, the id an answer names, and when it closes. */
export type PopupPrompt = meet_ai_lib_detection_popup_PopupPrompt;
/** A prompt ("Record this meeting?") or a countdown ("Zoom call ended", TUR-147). */
export type PopupCard = meet_ai_lib_detection_popup_Card;
export type PopupAnswer = meet_ai_lib_detection_popup_PopupAnswer;

/** The prompt on screen, for a popup that just loaded. */
export async function promptPopupCurrent(): Promise<PopupPrompt | null> {
  if (!hasBackend()) return null;
  return call(() => commands.promptPopupCurrent());
}

/** A popup button was pressed. Rust closes the popup and does what it says. */
export async function answerPromptPopup(id: number, answer: PopupAnswer): Promise<void> {
  await call(() => commands.answerPromptPopup(id, answer));
}

/** A new prompt for the popup; returns the unsubscribe. */
export function onPromptPopup(handler: (shown: PopupPrompt) => void): () => void {
  // Nothing to listen to without a backend: the popup shows what
  // `promptPopupCurrent` said. A failed listen is logged (TUR-173).
  return subscribe(PROMPT_POPUP_EVENT, handler);
}
