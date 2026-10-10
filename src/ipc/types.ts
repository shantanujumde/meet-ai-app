/**
 * The shapes that cross the IPC boundary, under the names the screens use.
 *
 * Every type here is the generated one from `./bindings` (`just bindings`),
 * never a hand-written copy (TUR-173). So a field Rust renames, or a value it
 * adds to an enum, fails `pnpm typecheck` at every place that reads it,
 * instead of showing up as `undefined` at runtime. The doc comments on each
 * field are in `bindings.ts`, from the Rust side.
 */

import type {
  meet_ai_lib_agent_run_Failure,
  meet_ai_lib_agent_run_FailureKind,
  meet_ai_lib_agent_run_MeetingNotes,
  meet_ai_lib_agent_run_NotesSection,
  meet_ai_lib_agent_run_State,
  meet_ai_lib_agent_run_Status,
  meet_ai_lib_agent_setup_AgentChoice,
  meet_ai_lib_agent_setup_AgentCli,
  meet_ai_lib_agent_setup_AgentCliId,
  meet_ai_lib_agent_setup_AgentCliState,
  meet_ai_lib_agent_setup_AgentHarness,
  meet_ai_lib_agent_setup_AgentModel,
  meet_ai_lib_agent_setup_AgentTestResult,
  meet_ai_lib_agent_setup_AgentTestTask,
  meet_ai_lib_brief_BriefTicket,
  meet_ai_lib_brief_Commit,
  meet_ai_lib_brief_MeetingBrief,
  meet_ai_lib_brief_PreviousMeeting,
  meet_ai_lib_brief_RepoCommits,
  meet_ai_lib_engine_choices_EngineAvailability,
  meet_ai_lib_engine_choices_EngineChoice,
  meet_ai_lib_engine_choices_EngineChoices,
  meet_ai_lib_engine_choices_SpokenLanguageOption,
  meet_ai_lib_engine_EnvironmentView,
  meet_ai_lib_engine_ModelView,
  meet_ai_lib_engine_ProgressEvent,
  meet_ai_lib_engine_parakeet_ModelCredit,
  meet_ai_lib_engine_parakeet_ParakeetModelView,
  meet_ai_lib_engine_SelectionView,
  meet_ai_lib_error_UiError,
  meet_ai_lib_live_transcript_Snapshot,
  meet_ai_lib_live_transcript_State,
  meet_ai_lib_live_transcript_Status,
  meet_ai_lib_meetings_list_MeetingList,
  meet_ai_lib_meetings_view_MeetingDetail,
  meet_ai_lib_meetings_view_MeetingSummary,
  meet_ai_lib_meetings_view_TranscriptLine,
  meet_ai_lib_onboarding_State,
  meet_ai_lib_permission_Pane,
  meet_ai_lib_permission_State,
  meet_ai_lib_permission_Status,
  meet_ai_lib_recording_phase_Phase,
  meet_ai_lib_recording_Status,
  meet_ai_lib_recording_state_RecordingState,
  meet_ai_lib_sync_tracker_ServerStatus,
  meet_ai_lib_sync_tracker_Tracker,
  meet_ai_lib_sync_tracker_TrackerServer,
  meet_ai_lib_sync_tracker_TrackerSettings,
  meet_ai_lib_tickets_TicketSummary,
  meet_ai_lib_watch_Changed,
  meeting_format_Speaker,
  store_index_Hit,
  store_ticket_Status,
  stt_model_ModelTag,
  stt_session_LiveLine,
  stt_session_LiveUpdate,
} from "./bindings";

/**
 * An error from Rust.
 *
 * `domain` keeps `stt::Error` and `modelfetch::Error` apart: they are two
 * enums on purpose, and a checksum failure and a dead sidecar need different
 * words and a different button.
 *
 * Branch on `domain` + `kind`. Never on `message`: that is the Rust error's own
 * sentence, shown verbatim so the user can copy it into a bug report, and
 * matching on it turns every reworded string into a silently broken screen.
 */
export type UiError = meet_ai_lib_error_UiError;

// --- meetings -------------------------------------------------------------

/** A row in the meeting list. */
export type MeetingSummary = meet_ai_lib_meetings_view_MeetingSummary;
/** How a meeting's recording ended (TUR-97, TUR-145, SPEC §3.1). */
export type RecordingState = meet_ai_lib_recording_state_RecordingState;
/** One line of `transcript.md`, parsed per SPEC §3.4. */
export type TranscriptLine = meet_ai_lib_meetings_view_TranscriptLine;
export type MeetingDetail = meet_ai_lib_meetings_view_MeetingDetail;
export type MeetingList = meet_ai_lib_meetings_list_MeetingList;
/** Payload of `meetings-changed`: files in the meetings folder changed outside the app. */
export type MeetingsChanged = meet_ai_lib_watch_Changed;
/** One meeting-search result. `snippet` is plain text with matches wrapped in « and ». */
export type SearchHit = store_index_Hit;

// --- permission and onboarding ----------------------------------------------

export type PermissionState = meet_ai_lib_permission_State;
export type PermissionStatus = meet_ai_lib_permission_Status;
/** Which System Settings pane to deep-link to. */
export type PrivacyPane = meet_ai_lib_permission_Pane;
export type OnboardingState = meet_ai_lib_onboarding_State;

// --- engine and models ------------------------------------------------------

/** What the filesystem says about the speech engines. Cheap to fetch. */
export type EnvironmentView = meet_ai_lib_engine_EnvironmentView;
/** Which engine will actually be used. Costs a ~160 ms subprocess probe. */
export type SelectionView = meet_ai_lib_engine_SelectionView;
export type ModelView = meet_ai_lib_engine_ModelView;
/** One word about a model, drawn as a chip. Labels live in `ui/engine/tags.ts`. */
export type ModelTag = stt_model_ModelTag;
/** `transcription.engine` in config.jsonc. */
export type EngineChoice = meet_ai_lib_engine_choices_EngineChoice;
/** Whether one engine can be picked; `reason` says why not, in a sentence. */
export type EngineAvailability = meet_ai_lib_engine_choices_EngineAvailability;
/** The Settings engine picker (TUR-75). Costs the ~160 ms probe. */
export type EngineChoices = meet_ai_lib_engine_choices_EngineChoices;
/** One language whisper can be told the audio is in. */
export type SpokenLanguageOption = meet_ai_lib_engine_choices_SpokenLanguageOption;
/** The Parakeet model (TUR-62): one download of several files. */
export type ParakeetModelView = meet_ai_lib_engine_parakeet_ParakeetModelView;
/** A model licence Settings, About credits (TUR-62). */
export type ModelCredit = meet_ai_lib_engine_parakeet_ModelCredit;
/** One `model://progress` event. */
export type ModelProgress = meet_ai_lib_engine_ProgressEvent;

// --- recording and the live transcript --------------------------------------

export type RecordingPhase = meet_ai_lib_recording_phase_Phase;
/**
 * The recorder's state. `error` says why the last recording ended badly or
 * the last start was refused (TUR-97, TUR-127); only set on an idle status.
 */
export type RecordingStatus = meet_ai_lib_recording_Status;
/**
 * Who a live line belongs to: `you` is the mic track, `others` the system
 * track. Lower-case on the wire; the pane renders them as §3.4's `You`/`Others`.
 */
export type LiveSpeaker = meeting_format_Speaker;
/**
 * One line in the live pane, volatile or settled. `start_sec` is snake_case:
 * the TUR-96 event contract, not a slip from the camelCase rule.
 */
export type LiveLine = stt_session_LiveLine;
/**
 * One `transcript://update` event: `volatile` replaces that speaker's
 * in-progress guess, `final` settles it into a line, and `dropped` clears it
 * without a line.
 */
export type TranscriptUpdate = stt_session_LiveUpdate;
export type TranscriptState = meet_ai_lib_live_transcript_State;
/** One `transcript://status` event. */
export type TranscriptStatus = meet_ai_lib_live_transcript_Status;
/** What `live_transcript` returns: everything so far, for a window opened mid-meeting. */
export type LiveTranscriptSnapshot = meet_ai_lib_live_transcript_Snapshot;

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

// --- tickets and Sync -------------------------------------------------------

export type TicketStatus = store_ticket_Status;
export type TicketSummary = meet_ai_lib_tickets_TicketSummary;
/** The issue trackers Sync can create an issue in. */
export type Tracker = meet_ai_lib_sync_tracker_Tracker;
/** Which agent runs Sync; `none` means the user chose no agent. */
export type Harness = meet_ai_lib_agent_setup_AgentHarness;
/** Where Sync sends a ticket, and which agent does it. */
export type TrackerSettings = meet_ai_lib_sync_tracker_TrackerSettings;
/** An MCP server's state, as `claude mcp list` / `codex mcp list` report it. */
export type McpStatus = meet_ai_lib_sync_tracker_ServerStatus;
/** One MCP server the agent knows about. */
export type TrackerServer = meet_ai_lib_sync_tracker_TrackerServer;

// --- notes runs (TUR-10) ----------------------------------------------------

/** Every way a notes run can end without notes. Branch on this, never on the message. */
export type NotesRunFailureKind = meet_ai_lib_agent_run_FailureKind;
/** Why a run wrote no notes, in plain words. */
export type NotesRunFailure = meet_ai_lib_agent_run_Failure;
/** Where a meeting's notes run is. `idle`: no run since launch, so go by the disk. */
export type NotesRunState = meet_ai_lib_agent_run_State;
/** What `notes_run_status` and the agent-run status event carry. */
export type NotesRunStatus = meet_ai_lib_agent_run_Status;
/** One agent-written section of `meeting.md`. `body` is markdown as written. */
export type NotesSection = meet_ai_lib_agent_run_NotesSection;
/** The agent-written half of `meeting.md`, for the meeting view. */
export type MeetingNotes = meet_ai_lib_agent_run_MeetingNotes;

// --- agent setup (SPEC A11, the Setup row) ---------------------------------

/** Which agent writes the notes. `none` is the copy-prompt fallback. */
export type AgentHarness = meet_ai_lib_agent_setup_AgentHarness;
/** `agent` in config.jsonc: the chosen agent, its model, and an optional path to its CLI. */
export type AgentChoice = meet_ai_lib_agent_setup_AgentChoice;
/** The agents meet-ai can run itself. */
export type AgentCliId = meet_ai_lib_agent_setup_AgentCliId;
/** `ready`: installed and signed in; `signed-out`: installed only; `missing`: not found. */
export type AgentCliState = meet_ai_lib_agent_setup_AgentCliState;
/** One agent CLI, as found on this machine. */
export type AgentCli = meet_ai_lib_agent_setup_AgentCli;
/** One model the picker offers. */
export type AgentModel = meet_ai_lib_agent_setup_AgentModel;
/** One task from a test run. */
export type AgentTestTask = meet_ai_lib_agent_setup_AgentTestTask;
/** What the Test button's sample run gave back. */
export type AgentTestResult = meet_ai_lib_agent_setup_AgentTestResult;

// --- the pre-meeting brief (TUR-32) -------------------------------------------

/** One ticket still open from the last meeting with the same title. */
export type BriefTicket = meet_ai_lib_brief_BriefTicket;
/** The last meeting with the same title. */
export type PreviousMeeting = meet_ai_lib_brief_PreviousMeeting;
/** One `git log` line: short hash and subject. */
export type BriefCommit = meet_ai_lib_brief_Commit;
/** What landed in the meeting's repo since last time. */
export type RepoCommits = meet_ai_lib_brief_RepoCommits;
/** What was said last time and the commits since; `previous` null when none. */
export type MeetingBrief = meet_ai_lib_brief_MeetingBrief;
