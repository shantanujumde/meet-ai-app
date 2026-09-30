/**
 * The shapes that cross the IPC boundary.
 *
 * Every type here mirrors a `#[derive(Serialize)]` struct in `src-tauri/src/`,
 * which uses `#[serde(rename_all = "camelCase")]`, so the field names match
 * exactly. The two sides are a pair: change a Rust struct and change this file
 * in the same commit, or the mismatch shows up as `undefined` at runtime
 * instead of as a type error at build time.
 */

/**
 * An error from Rust.
 *
 * `domain` keeps `stt::Error` and `modelfetch::Error` apart — they are two
 * enums on purpose, and a checksum failure and a dead sidecar need different
 * words and a different button.
 *
 * Branch on `domain` + `kind`. Never on `message`: that is the Rust error's own
 * sentence, shown verbatim so the user can copy it into a bug report, and
 * matching on it turns every reworded string into a silently broken screen.
 */
export type UiError = {
  domain: "stt" | "model" | "app";
  kind: string;
  message: string;
};

/** A row in the meeting list. */
export type MeetingSummary = {
  /** The folder name, e.g. `2026-09-01-1430-standup`. Also the route param. */
  id: string;
  title: string;
  /** `YYYY-MM-DD`, or null for a folder that was renamed by hand. */
  date: string | null;
  /** `HH:MM`. */
  time: string | null;
  lineCount: number;
  /** The timestamp on the last line — how far into the meeting it runs. */
  lastTimestamp: string | null;
  hasNotes: boolean;
  /** `meeting.md` exists, i.e. an agent has wrapped this meeting up. */
  hasAnalysis: boolean;
  /** How the recording ended. See {@link RecordingState}. */
  recordingState: RecordingState;
  /**
   * Milliseconds of audio a player can reach — the longer track's WAV header
   * (SPEC A5). Null when the folder has no audio at all.
   */
  audioMs: number | null;
};

/**
 * How a meeting's recording ended (TUR-97, SPEC §3.1).
 *
 * - `finished` — stopped on purpose, or there is no audio to judge by.
 * - `interrupted` — cut short by a force quit, a crash or the Mac shutting
 *   down. Shown as "Interrupted" and opened exactly like any other meeting.
 * - `recording` — this app is writing it right now.
 */
export type RecordingState = "finished" | "interrupted" | "recording";

/** One line of `transcript.md`, parsed per SPEC §3.4. */
export type TranscriptLine = {
  /** 0-based line number in the file. */
  seq: number;
  /** `HH:MM:SS`, the utterance start. */
  time: string;
  speaker: "You" | "Others";
  text: string;
};

export type MeetingDetail = {
  summary: MeetingSummary;
  /** Absolute path on disk, so the UI can say where the files are. */
  path: string;
  lines: TranscriptLine[];
  /** No `transcript.md` at all — a different state from "it is empty". */
  transcriptMissing: boolean;
  /** Lines that did not match §3.4 and were skipped. */
  unparsedLineCount: number;
  notes: string;
};

export type MeetingList = {
  root: string;
  /** Before the first recording the folder does not exist, and that is fine. */
  rootExists: boolean;
  meetings: MeetingSummary[];
};

export type PermissionState = "unknown" | "granted" | "denied";

export type PermissionStatus = {
  state: PermissionState;
  /** False means nobody has actually checked — not that the answer is no. */
  measured: boolean;
  detail: string;
};

/** Which System Settings pane to deep-link to. */
export type PrivacyPane = "audio-capture" | "microphone";

export type OnboardingState = {
  /** RFC 3339, or null if onboarding has not been finished. */
  completedAt: string | null;
};

/** What the filesystem says about the speech engines. Cheap to fetch. */
export type EnvironmentView = {
  sidecar: string | null;
  whisperModel: string | null;
  locale: string;
  modelId: string;
  modelsDir: string | null;
};

/** Which engine will actually be used. Costs a ~160 ms subprocess probe. */
export type SelectionView = {
  engine: "apple-speech" | "whisper";
  reason: string;
};

export type ModelView = {
  id: string;
  filename: string;
  /** The pinned size in bytes. Read this, do not hardcode a number. */
  bytes: number;
  installed: boolean;
};

export type ModelProgress = {
  modelId: string;
  downloadedBytes: number;
  totalBytes: number;
  /** All bytes are here; the SHA-256 is being computed. No sub-progress. */
  verifying: boolean;
};

export type RecordingPhase = "idle" | "starting" | "recording" | "stopping";

export type RecordingStatus = {
  phase: RecordingPhase;
  meetingId: string | null;
  /** Unix epoch ms, so the UI runs its own timer instead of being fed ticks. */
  startedAtMs: number | null;
};

/** Narrow an unknown thrown value to a {@link UiError}. */
export function isUiError(value: unknown): value is UiError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<UiError>;
  return (
    typeof candidate.domain === "string" &&
    typeof candidate.kind === "string" &&
    typeof candidate.message === "string"
  );
}

/**
 * Coerce anything thrown into a {@link UiError}.
 *
 * A `catch` can receive a plain `Error`, a string, or whatever a plugin threw.
 * Every one of those still has to reach a screen with a human sentence next to
 * it, so nothing is allowed to fall through as "[object Object]".
 */
export function toUiError(value: unknown): UiError {
  if (isUiError(value)) return value;
  if (value instanceof Error) {
    return { domain: "app", kind: "unexpected", message: value.message };
  }
  if (typeof value === "string" && value.length > 0) {
    return { domain: "app", kind: "unexpected", message: value };
  }
  return {
    domain: "app",
    kind: "unexpected",
    message: "Something went wrong, and meet-ai did not get a reason why.",
  };
}
