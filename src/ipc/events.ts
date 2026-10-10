/**
 * Every Tauri event the window listens to, and the payload each carries
 * (TUR-173).
 *
 * The names are the generated constants from `src-tauri/src/events.rs` (rule
 * R2) and the payloads are the generated Rust types, so `subscribe` ties the
 * two together: a renamed payload field, or a handler written for the wrong
 * event, fails `pnpm typecheck`. A new event in `events.rs` with no entry here
 * fails it too ({@link CheckedEventMap}).
 *
 * Re-exported from `./client`; import from there.
 */

import { type Event, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { warn } from "@tauri-apps/plugin-log";
import type * as bindings from "./bindings";
import {
  AGENT_RUN_STATUS_EVENT,
  DETECTION_PROMPT_EVENT,
  HEADPHONE_WARNING_EVENT,
  HOOK_FAILED_EVENT,
  MEETINGS_CHANGED_EVENT,
  MEETINGS_WATCH_PROBLEM_EVENT,
  MODEL_PROGRESS_EVENT,
  type meet_ai_lib_agent_run_Status,
  type meet_ai_lib_detection_notify_Prompt,
  type meet_ai_lib_detection_popup_PopupPrompt,
  type meet_ai_lib_engine_ProgressEvent,
  type meet_ai_lib_headphone_warning_HeadphoneWarning,
  type meet_ai_lib_hooks_app_HookFailed,
  type meet_ai_lib_lifecycle_NavigateTo,
  type meet_ai_lib_live_transcript_Status,
  type meet_ai_lib_permission_Status,
  type meet_ai_lib_recording_Status,
  type meet_ai_lib_sync_auto_TicketSyncStatus,
  type meet_ai_lib_watch_Changed,
  type meet_ai_lib_watch_WatchProblem,
  NAVIGATE_EVENT,
  PERMISSION_STATUS_EVENT,
  PROMPT_POPUP_EVENT,
  QUIT_CONFIRM_EVENT,
  RECORDING_STATE_EVENT,
  type stt_session_LiveUpdate,
  TICKET_SYNC_EVENT,
  TRANSCRIPT_STATUS_EVENT,
  TRANSCRIPT_UPDATE_EVENT,
} from "./bindings";
import { hasBackend } from "./client";

/** Each event's name, keyed to the Rust type it is emitted with. */
export type EventMap = {
  [RECORDING_STATE_EVENT]: meet_ai_lib_recording_Status;
  [MODEL_PROGRESS_EVENT]: meet_ai_lib_engine_ProgressEvent;
  [PERMISSION_STATUS_EVENT]: meet_ai_lib_permission_Status;
  [TRANSCRIPT_UPDATE_EVENT]: stt_session_LiveUpdate;
  [TRANSCRIPT_STATUS_EVENT]: meet_ai_lib_live_transcript_Status;
  [MEETINGS_CHANGED_EVENT]: meet_ai_lib_watch_Changed;
  [AGENT_RUN_STATUS_EVENT]: meet_ai_lib_agent_run_Status;
  [DETECTION_PROMPT_EVENT]: meet_ai_lib_detection_notify_Prompt;
  /** Emitted with `()`, which arrives as `null`. */
  [QUIT_CONFIRM_EVENT]: null;
  [NAVIGATE_EVENT]: meet_ai_lib_lifecycle_NavigateTo;
  [PROMPT_POPUP_EVENT]: meet_ai_lib_detection_popup_PopupPrompt;
  [HOOK_FAILED_EVENT]: meet_ai_lib_hooks_app_HookFailed;
  [HEADPHONE_WARNING_EVENT]: meet_ai_lib_headphone_warning_HeadphoneWarning;
  [MEETINGS_WATCH_PROBLEM_EVENT]: meet_ai_lib_watch_WatchProblem;
  [TICKET_SYNC_EVENT]: meet_ai_lib_sync_auto_TicketSyncStatus;
};

/** Every event name `bindings.ts` exports: its `*_EVENT` constants. */
type EventName = (typeof bindings)[Extract<keyof typeof bindings, `${string}_EVENT`>];

/** Fails typecheck when `events.rs` has an event {@link EventMap} does not name. */
type Complete<T extends Record<EventName, unknown>> = T;
export type CheckedEventMap = Complete<EventMap>;

/**
 * Subscribe to a Tauri event, returning an unsubscribe function.
 *
 * The `listen` call is async but React effects need a synchronous cleanup, so
 * the returned function tears down whichever of the two wins the race: an
 * effect that unmounts before `listen` resolves must still not leak a listener.
 */
export function subscribe<E extends keyof EventMap>(
  event: E,
  onEvent: (payload: EventMap[E]) => void,
): () => void {
  let unlisten: UnlistenFn | undefined;
  let cancelled = false;

  if (hasBackend()) {
    listen<EventMap[E]>(event, (received: Event<EventMap[E]>) => onEvent(received.payload))
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((error: unknown) => logListenFailure(event, error));
  }

  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/**
 * A `listen` that failed (a window missing the capability, say) means that
 * window's live updates stop. The caller's state stays at its last explicit
 * fetch, which is the right fallback, but it must not be silent: say so in
 * the app log, or in the console when the log cannot be written either.
 */
function logListenFailure(event: string, error: unknown): void {
  const message = `could not listen for ${event}; its updates will not arrive: ${String(error)}`;
  warn(message).catch(() => console.warn(message));
}
