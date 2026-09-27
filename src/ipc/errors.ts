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

import type { UiError } from "./types";

/** What the button does. The screen decides how; this decides which. */
export type Remedy =
  | { action: "download-model" }
  | { action: "copy-details" }
  | { action: "retry" }
  | { action: "redownload" }
  | { action: "open-settings" }
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
    body: "Apple's built-in speech engine is not available here, so meet-ai needs the whisper model instead. It is a one-time download and everything stays on this Mac.",
    actionLabel: "Download",
    remedy: { action: "download-model" },
  },
  "stt/sidecar": {
    headline: "Transcription stopped. Your recording is still safe.",
    body: "The speech helper stopped working. The audio and everything already transcribed are on disk and untouched; only new text has stopped arriving.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "stt/engine": {
    headline: "Transcription stopped. Your recording is still safe.",
    body: "The speech engine failed partway through. The audio and everything already transcribed are on disk and untouched; only new text has stopped arriving.",
    actionLabel: "Copy details",
    remedy: { action: "copy-details" },
  },
  "stt/model-missing": {
    headline: "The speech model is not on this Mac",
    body: "meet-ai looked for the whisper model file and it was not there. Downloading it again puts it back.",
    actionLabel: "Download",
    remedy: { action: "download-model" },
  },
  "stt/streaming-unsupported": {
    headline: "This engine cannot caption a meeting as it happens",
    body: "It can still transcribe the recording once you stop. Nothing is lost — the text just arrives at the end instead of live.",
    actionLabel: null,
    remedy: { action: "none" },
  },

  // --- modelfetch::Error --------------------------------------------------
  "model/download": {
    headline: "The download did not finish",
    body: "Usually this is the network rather than anything wrong on this Mac. meet-ai keeps what it already downloaded, so trying again picks up where it stopped.",
    actionLabel: "Try again",
    remedy: { action: "retry" },
  },
  "model/checksum": {
    headline: "The downloaded file is not the one meet-ai expected",
    body: "Every byte arrived, but the file's fingerprint does not match the one meet-ai has pinned for this model. That can mean a corrupted download — or a file that was tampered with in transit. meet-ai has deleted it rather than load it, so nothing unverified ever runs.",
    actionLabel: "Download again from scratch",
    remedy: { action: "redownload" },
    security: true,
  },

  // --- the app shell itself ------------------------------------------------
  "app/permission-denied": {
    headline: "meet-ai is not allowed to record this Mac's audio",
    body: "Recording now would capture nothing but silence. You can change this in System Settings, under Privacy & Security.",
    actionLabel: "Open System Settings",
    remedy: { action: "open-settings" },
  },
  "app/meeting-not-found": {
    headline: "That meeting is not on disk any more",
    body: "Its folder has been moved, renamed or deleted outside meet-ai. The list refreshes when you go back to it.",
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
    body: "Only one download per model runs at a time, so the two cannot overwrite each other's progress.",
    actionLabel: null,
    remedy: { action: "none" },
  },
};

/** The copy for an error. Never throws, and never returns nothing. */
export function copyFor(error: UiError): ErrorCopy {
  return COPY[`${error.domain}/${error.kind}`] ?? GENERIC;
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
