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
  /**
   * `meeting.md` says `agent_notes: off`: the user switched notes off for
   * this meeting (TUR-12, SPEC A11), so its transcript is never sent to an
   * agent. The list marks it "Notes off".
   */
  notesOff: boolean;
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
  /** The grants that are off, by the Settings pane each lives in. Empty unless denied. */
  denied: PrivacyPane[];
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
  /**
   * Why the last recording ended badly — it stopped on its own (a checkpoint
   * or a device change failed), or did not close cleanly — or why the last
   * start was refused, including one from ⌘⇧R or the menu bar (TUR-127). Only
   * set on an idle status; cleared by the next start. Rust always sends it,
   * as `null` when nothing went wrong.
   */
  error: UiError | null;
};

/**
 * Who a live line belongs to: `you` is the mic track, `others` the system
 * track. Lower-case on the wire; the pane renders them as §3.4's `You`/`Others`.
 */
export type LiveSpeaker = "you" | "others";

/**
 * One line in the live pane, volatile or settled.
 *
 * The one snake_case field in this file: `start_sec` is the TUR-96 event
 * contract as agreed with `src-tauri`, not a slip from the camelCase rule above.
 */
export type LiveLine = {
  /** Meeting-global and monotonic. Stable enough to be the React key. */
  seq: number;
  speaker: LiveSpeaker;
  /** Utterance start, in seconds from the start of the recording (§3.4). */
  start_sec: number;
  text: string;
};

/**
 * One `transcript://update` event.
 *
 * `volatile` replaces that speaker's in-progress guess, `final` settles it into
 * a line, and `dropped` means the guess came to nothing — the recognizer threw
 * it away, or the silence guard (TUR-67) did — so it clears without a line.
 */
export type TranscriptUpdate =
  | ({ kind: "volatile" | "final" } & LiveLine)
  | { kind: "dropped"; speaker: LiveSpeaker; seq: number };

export type TranscriptState = "idle" | "running" | "stopped" | "failed";

/** One `transcript://status` event. */
export type TranscriptStatus = {
  state: TranscriptState;
  /** Which engine is transcribing, e.g. `apple-speech` or `whisper`. */
  engine: string | null;
  /** A sentence for the user when `state` is `failed`. */
  detail: string | null;
};

/**
 * What `live_transcript` returns: everything so far, so a window opened
 * mid-meeting catches up before the events carry on from there.
 */
export type LiveTranscriptSnapshot = {
  status: TranscriptStatus;
  finals: LiveLine[];
  volatile: LiveLine[];
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

/** Payload of `meetings-changed`: files in the meetings folder changed outside the app. */
export interface MeetingsChanged {
  paths: string[];
}

/** One meeting-search result. `snippet` is plain text with matches wrapped in « and ». */
export type SearchHit = {
  meetingId: string;
  title: string;
  date: string | null;
  snippet: string;
  /** Like "00:12:03"; null when the hit is a title or notes match. */
  timestamp: string | null;
};

export type TicketStatus = "open" | "in_progress" | "done" | "dropped";

export type TicketSummary = {
  id: string;
  title: string;
  /** Null when the ticket file has no recognisable status. */
  status: TicketStatus | null;
  /** The meeting id this ticket came from, if any. */
  meeting: string | null;
  body: string;
  hasProblems: boolean;
  /**
   * Which tracker the agent created this ticket's issue in (`linear`, `jira`
   * or `github` in practice), or null before it has been synced (TUR-11).
   */
  syncedTo: string | null;
  /** The issue's id in that tracker, e.g. `ENG-42`. */
  externalId: string | null;
  /** The issue's web address. Rust opens it; the window never does. */
  externalUrl: string | null;
};

/** The issue trackers Sync can create an issue in. */
export type Tracker = "linear" | "jira" | "github";

/** Which agent runs Sync; `none` means the user chose no agent. */
export type Harness = "claude-code" | "codex" | "none";

/** Where Sync sends a ticket, and which agent does it. */
export type TrackerSettings = {
  tracker: Tracker;
  /** The name of the tracker's MCP server, as the agent lists it. */
  trackerMcp: string;
  harness: Harness;
};

/** An MCP server's state, as `claude mcp list` / `codex mcp list` report it. */
export type McpStatus =
  | "connected"
  | "needs_auth"
  | "failed"
  | "pending"
  | "disabled"
  | "configured"
  | "unknown";

/** One MCP server the agent knows about. */
export type TrackerServer = {
  name: string;
  status: McpStatus;
};

/**
 * Every way a notes run can end without notes (TUR-10). One per thing the user
 * can do about it. Branch on this, never on `NotesRunFailure.message`.
 */
export type NotesRunFailureKind =
  | "no-agent"
  | "not-installed"
  | "not-signed-in"
  | "timed-out"
  | "cancelled"
  | "cli-failed"
  | "bad-reply"
  | "could-not-start"
  | "no-transcript"
  | "notes-off"
  | "write-failed";

/** Why a run wrote no notes, in plain words. */
export type NotesRunFailure = {
  kind: NotesRunFailureKind;
  /** One or two sentences for the meeting view, next to Retry. Shown as is. */
  message: string;
  /** Something to type in Terminal that fixes it (signing in), when there is one. */
  command: string | null;
};

/**
 * Where a meeting's notes run is. `idle` means no run for this meeting since
 * launch, so the view goes by what is on disk.
 */
export type NotesRunState =
  | { state: "idle" }
  | { state: "running" }
  /** `tasks` is how many tickets were written. */
  | { state: "done"; tasks: number }
  | { state: "failed"; failure: NotesRunFailure };

/** What `notes_run_status` and the agent-run status event carry. */
export type NotesRunStatus = {
  meetingId: string;
  state: NotesRunState;
};

/** One agent-written section of `meeting.md`. `body` is markdown as written. */
export type NotesSection = {
  heading: string;
  body: string;
};

/** The agent-written half of `meeting.md`, for the meeting view. */
export type MeetingNotes = {
  /** The meeting is marked `agent_notes: off`. */
  notesOff: boolean;
  /** `claude-code`, `codex`, `clipboard`; null before any notes were written. */
  analyzedBy: string | null;
  /** Summary, Decisions, Action Items, Open Questions — the ones with text. */
  sections: NotesSection[];
};

// --- agent setup (SPEC A11, the Setup row) ---------------------------------

/** Which agent writes the notes. `none` is the copy-prompt fallback. */
export type AgentHarness = "claude-code" | "codex" | "none";

/** `agent` in config.jsonc: the chosen agent, its model, and an optional path to its CLI. */
export type AgentChoice = {
  harness: AgentHarness;
  /** Any model name the chosen CLI accepts. Blank means the CLI's own default. */
  model: string;
  /** Set when the automatic lookup cannot find the CLI. Null means look it up. */
  binaryPath: string | null;
};

/** The agents meet-ai can run itself. */
export type AgentCliId = "claude-code" | "codex";

/**
 * What detection found: `ready` is installed and signed in, `signed-out` is
 * installed but not signed in, `missing` is not found at all.
 */
export type AgentCliState = "ready" | "signed-out" | "missing";

/** One agent CLI, as found on this Mac. */
export type AgentCli = {
  id: AgentCliId;
  /** "Claude Code" or "Codex". */
  name: string;
  /** "Anthropic" or "OpenAI": who the transcript is sent to. */
  provider: string;
  state: AgentCliState;
  /** Where it was found. Null when missing. */
  path: string | null;
  version: string | null;
  /** What to run in Terminal to sign in, e.g. `claude auth login`. */
  signInCommand: string;
  /** Model names to suggest. Free text is still allowed. */
  models: string[];
  /** "opus" for Claude Code. Null means the CLI's own default. */
  defaultModel: string | null;
  /** Whether the Test button can run this agent. False when it is missing. */
  canTest: boolean;
};

/** One task from a test run. */
export type AgentTestTask = { title: string; owner: string | null; due: string | null };

/** What the Test button's sample run gave back. */
export type AgentTestResult = {
  summary: string;
  decisions: string[];
  openQuestions: string[];
  tasks: AgentTestTask[];
  /** Wall time of the run, in seconds. */
  seconds: number;
};
