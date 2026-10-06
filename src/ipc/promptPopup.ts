/**
 * The prompt card window's IPC (TUR-59, TUR-108): its prompt, its
 * answers, and the event that brings a new prompt. Its own file so
 * `client.ts` stays under the size limit; same rules as there: without a
 * backend, reads return nothing and writes reject with `no-backend`.
 */

import { listen } from "@tauri-apps/api/event";
import {
  commands,
  type meet_ai_lib_detection_popup_PopupAnswer,
  type meet_ai_lib_detection_popup_PopupPrompt,
  PROMPT_POPUP_EVENT,
} from "./bindings";
import { hasBackend } from "./client";
import { NO_BACKEND } from "./errors";
import { toUiError } from "./types";

/** What the popup shows: the prompt, and the id an answer names. */
export type PopupPrompt = meet_ai_lib_detection_popup_PopupPrompt;
export type PopupAnswer = meet_ai_lib_detection_popup_PopupAnswer;

/** The prompt on screen, for a popup that just loaded. */
export async function promptPopupCurrent(): Promise<PopupPrompt | null> {
  if (!hasBackend()) return null;
  return commands.promptPopupCurrent();
}

/** A popup button was pressed. Rust closes the popup and does what it says. */
export async function answerPromptPopup(id: number, answer: PopupAnswer): Promise<void> {
  if (!hasBackend()) throw NO_BACKEND;
  let result: Awaited<ReturnType<typeof commands.answerPromptPopup>>;
  try {
    result = await commands.answerPromptPopup(id, answer);
  } catch (thrown) {
    throw toUiError(thrown);
  }
  if (result.status === "error") throw toUiError(result.error);
}

/** A new prompt for the popup; returns the unsubscribe. */
export function onPromptPopup(handler: (shown: PopupPrompt) => void): () => void {
  if (!hasBackend()) return () => {};
  let stop: (() => void) | undefined;
  let cancelled = false;
  listen<PopupPrompt>(PROMPT_POPUP_EVENT, (event) => handler(event.payload))
    .then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    })
    .catch(() => {
      // Nothing to listen to; the popup shows what `promptPopupCurrent` said.
    });
  return () => {
    cancelled = true;
    stop?.();
  };
}
