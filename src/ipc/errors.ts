/**
 * Turning a Rust error into something a person can act on.
 *
 * The mapping below is the table agreed with Vox on TUR-6. Two rules make it
 * work, and both are easy to break by accident:
 *
 * 1. **Discriminate on `domain` + `kind`, never on `message`.** `stt::Error`
 *    and `modelfetch::Error` are deliberately separate enums; `kind` is their
 *    stable variant tag. Matching on the message means a reworded Rust string
 *    silently drops a screen back to the generic fallback.
 * 2. **Show `message` verbatim, next to the button.** The Rust errors are
 *    already whole sentences and they carry the specifics — which file, which
 *    digest, which exit code. The copy here says what it means and what to do;
 *    the message says what actually happened. A user pasting a bug report needs
 *    both.
 */

import { currentOs, type Os, osText } from "@/lib/osText";
import type { UiError } from "./types";

/**
 * The error for anything that needs Rust when there is no Rust behind the
 * window — `pnpm dev` in a plain browser, or a component test.
 *
 * One value, so every screen that hits it says the same thing: the folder
 * picker used to word its own copy, which read like a different problem.
 * `client.ts` throws this same value from its `call` guard.
 */
export const NO_BACKEND: UiError = {
  domain: "app",
  kind: "no-backend",
  message:
    "This is the meet-ai window running without its Mac app behind it, so it cannot read or " +
    "change anything on disk. Run `pnpm tauri dev` instead of `pnpm dev`.",
};

/**
 * The `app` kind for a damaged meetings-folder pointer (`root.json`, TUR-149).
 * Where the meetings are is unknown, and so is whether setup was done (that
 * flag lives in the meetings folder), so the app must not send the user back
 * through setup on it.
 */
export const ROOT_POINTER_UNREADABLE = "root-pointer-unreadable";

/** What the button does. The screen decides how; this decides which. */
export type Remedy =
  | { action: "download-model" }
  | { action: "copy-details" }
  | { action: "retry" }
  | { action: "redownload" }
  | { action: "open-settings" }
  | { action: "open-tracker-settings" }
  | { action: "none" };

export type ErrorCopy = {
  /** A short line naming what happened, in plain words. */
  headline: string;
  /** One or two sentences on what it means for the user. */
  body: string;
  /** Null when there is nothing useful to offer beyond the explanation. */
  actionLabel: string | null;
  remedy: Remedy;
  /**
   * True when the situation is a security concern rather than a glitch. The
   * screen styles these differently — a checksum mismatch should not look like
   * a flaky download.
   */
  security?: boolean;
};

const GENERIC: ErrorCopy = {
  headline: "Something went wrong",
  body: "meet-ai hit a problem it does not have specific advice for. The details below are what it knows.",
  actionLabel: "Copy details",
  remedy: { action: "copy-details" },
};

/** One wording for the three ways an agent's reply can be unusable. */
const AGENT_BAD_REPLY: ErrorCopy = {
  headline: "The agent's answer could not be used",
  body: "It replied, but not in the shape meet-ai asked for, so nothing was saved. Trying again usually works.",
  actionLabel: "Copy details",
  remedy: { action: "copy-details" },
};

/**
 * The agreed mapping, keyed by `domain/kind`.
 *
 * Keys that are not here fall through to {@link GENERIC}, which still shows the
 * message and still offers "Copy details" — so an error nobody anticipated is
 * never a dead end.
 */
const COPY: Record<string, ErrorCopy> = {
  // --- stt::Error ---------------------------------------------------------
  "stt/engine-unavailable": {
    headline: "This Mac will use the downloadable speech model",
    body: "Apple's built-in speech engine is not available here, so meet-ai needs the Whisper model instead. You download it once, and everything stays on this Mac.",
    actionLabel: "Download",
    remedy: { action: "download-model" },
  },
  "stt/sidecar": {
    headline: "Transcription stopped. Your recording is still safe.",
    body: "The speech helper stopped working. The audio and the text so far are saved and untouched. Only new text has stopped.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "stt/engine": {
    headline: "Transcription stopped. Your recording is still safe.",
    body: "The speech engine failed partway through. The audio and the text so far are saved and untouched. Only new text has stopped.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "stt/model-missing": {
    headline: "The speech model is not on this Mac",
    body: "meet-ai looked for the Whisper model file and could not find it. Download it again to put it back.",
    actionLabel: "Download",
    remedy: { action: "download-model" },
  },
  "stt/streaming-unsupported": {
    headline: "This engine cannot caption a meeting as it happens",
    body: "It can still transcribe the recording after you stop. Nothing is lost. The text just arrives at the end instead of live.",
    actionLabel: null,
    remedy: { action: "none" },
  },

  // --- modelfetch::Error --------------------------------------------------
  "model/download": {
    headline: "The download did not finish",
    body: "This is usually the network, not this Mac. meet-ai keeps what it already has, so trying again picks up where it stopped.",
    actionLabel: "Try again",
    remedy: { action: "retry" },
  },
  "model/checksum": {
    headline: "The downloaded file is not the one meet-ai expected",
    body: "The whole file arrived, but its checksum (a code that proves the file is the right one) does not match the one meet-ai expects for this model. The download may be damaged, or someone may have tampered with the file on the way. meet-ai deleted it without opening it, so nothing unchecked ever runs.",
    actionLabel: "Download again from scratch",
    remedy: { action: "redownload" },
    security: true,
  },

  // --- the app shell itself ------------------------------------------------
  "app/permission-denied": {
    headline: "meet-ai is not allowed to record this Mac's audio",
    body: "A recording now would be silent. You can allow it in System Settings, under Privacy & Security.",
    actionLabel: "Open System Settings",
    remedy: { action: "open-settings" },
  },
  "app/meeting-not-found": {
    headline: "That meeting is not on disk any more",
    body: "Its folder was moved, renamed or deleted outside meet-ai. The list updates when you go back to it.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/no-backend": {
    headline: "The Mac app is not running behind this window",
    body: "This is the interface on its own, with nothing to read or write files. Start it with `pnpm tauri dev`.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/download-already-running": {
    headline: "That model is already downloading",
    body: "Only one download per model runs at a time, so two downloads never overwrite each other.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/model-in-use": {
    headline: "That model is in use",
    body: "Pick another model first, and stop any recording.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/recording-in-progress": {
    headline: "Stop recording first",
    body: "meet-ai is still writing this meeting's files to the current folder. Stop the recording, then change the folder.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/folder-move-in-progress": {
    headline: "Your meetings folder is moving",
    body: "Try again when the move finishes.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/folder-busy": {
    headline: "Your meetings folder is in use",
    body: "meet-ai is still writing to it: a recording starting or stopping, notes being saved, or a model downloading. Change the folder when that finishes.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/same-folder": {
    headline: "That's already the folder",
    body: "Pick a different folder to move your meetings somewhere else.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/nested-folder": {
    headline: "That folder won't work",
    body: "A meetings folder can't be moved inside itself, or contain the folder it is moving from. Pick a folder outside the current one.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/folder-conflict": {
    headline: "Some meetings already exist there",
    body: "The folder you picked already has something with the same name as one of your meetings. meet-ai stopped so it would not overwrite anything.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "app/no-config-dir": {
    headline: "meet-ai could not remember that choice",
    body: "It could not find a place on this Mac to save the folder you chose, so it kept the old one. Nothing moved.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  // TUR-149: the folder checks and the pointer file.
  [`app/${ROOT_POINTER_UNREADABLE}`]: {
    headline: "meet-ai lost track of your meetings folder",
    body: "The file that remembers where your meetings are is damaged. Your meetings were not touched. Pick your meetings folder again under Settings, Files.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "app/relative-folder": {
    headline: "That folder won't work",
    body: "meet-ai needs the folder's full location. Pick the folder again.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/not-a-folder": {
    headline: "That is a file, not a folder",
    body: "Pick a folder to keep your meetings in. Nothing moved.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/unsupported-folder-name": {
    headline: "meet-ai can't remember that folder",
    body: "Its name has characters meet-ai cannot save. Pick or rename a folder with ordinary letters. Nothing moved.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },

  // --- the notes agent and the config that names it -------------------------
  "app/agent-not-installed": {
    headline: "The agent is not installed",
    body: "meet-ai could not find it. Install it, or pick another agent in Settings → Notes.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/agent-not-signed-in": {
    headline: "The agent is not signed in",
    body: "Sign in with the command shown in Settings → Notes, then try again.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/agent-timed-out": {
    headline: "The agent took too long",
    body: "meet-ai stopped waiting, and nothing from this run was saved. A long meeting can take a few minutes; try again.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "app/agent-cancelled": {
    headline: "Stopped",
    body: "The run was cancelled. Nothing from it was saved.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/agent-failed": {
    headline: "The agent stopped with an error",
    body: "Its own message is below. Nothing from this run was saved.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "app/agent-bad-reply": AGENT_BAD_REPLY,
  "app/agent-invalid-json": AGENT_BAD_REPLY,
  "app/agent-schema-mismatch": AGENT_BAD_REPLY,
  "app/agent-could-not-start": {
    headline: "The agent could not start",
    body: "meet-ai found it but could not run it. The details below say why.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  // --- sending a ticket to the tracker (TUR-113) ----------------------------
  "app/sync-unreachable": {
    headline: "Couldn't reach your tracker",
    body: "Your agent ran but could not reach the tracker, so no issue was made. Check that the tracker's connection is set up and signed in in your agent, then press Retry.",
    actionLabel: "Open Tracker settings",
    remedy: { action: "open-tracker-settings" },
  },
  "app/sync-refused": {
    headline: "The tracker refused the ticket",
    body: "It said why below. Check the project in Settings, Tracker, then press Retry.",
    actionLabel: "Open Tracker settings",
    remedy: { action: "open-tracker-settings" },
  },
  // TUR-155: a value in config.jsonc that is not valid.
  "app/invalid-config": {
    headline: "A setting in config.jsonc is not valid",
    body: "meet-ai shows that setting as its default and keeps every other setting. A file that cannot be read at all is never overwritten. Fix or remove the value named below in config.jsonc, in your meetings folder's .app folder.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "app/invalid-setting": {
    headline: "That value can't be saved",
    body: "It is outside what this setting allows. Nothing was changed.",
    actionLabel: null,
    remedy: { action: "none" },
  },
  "app/unknown-harness": {
    headline: "meet-ai does not know that agent",
    body: "config.jsonc names an agent meet-ai cannot run. Pick Claude Code, Codex or None in Settings → Notes.",
    actionLabel: null,
    remedy: { action: "none" },
  },
};

/**
 * "app/permission-denied" in the words of `os` (TUR-51). macOS keeps the
 * table's copy; Windows names its own Settings page. Linux never refuses on
 * permission, so it gets the plain wording.
 */
export function permissionDeniedCopy(os: Os): ErrorCopy {
  const mac = COPY["app/permission-denied"] as ErrorCopy;
  if (os === "macos") return mac;
  const settings = osText("settings", os);
  return {
    ...mac,
    headline: "meet-ai is not allowed to use the microphone",
    body:
      os === "windows"
        ? `Recording now would capture nothing from you. You can change this in ${settings}, under Privacy & security → Microphone.`
        : "Recording now would capture nothing from you. Check that a microphone is connected and not in use.",
    actionLabel: os === "windows" ? `Open ${settings}` : null,
    remedy: os === "windows" ? { action: "open-settings" } : { action: "none" },
  };
}

/** The copy for an error. Never throws, and never returns nothing. */
export function copyFor(error: UiError): ErrorCopy {
  const key = `${error.domain}/${error.kind}`;
  if (key === "app/permission-denied") return permissionDeniedCopy(currentOs());
  return COPY[key] ?? GENERIC;
}

/**
 * The text the "Copy details" button puts on the clipboard.
 *
 * Deliberately includes the machine-readable tag as well as the sentence. A
 * user pasting this into an issue should not have to describe which error it
 * was in their own words.
 */
export function detailsFor(error: UiError): string {
  return `meet-ai error\n${error.domain}/${error.kind}\n\n${error.message}`;
}
