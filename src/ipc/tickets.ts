/**
 * Tickets, a meeting's suggested tasks, and sending them to the tracker
 * (TUR-11, TUR-113).
 *
 * Re-exported from `./client`; import from there.
 */

import {
  commands,
  type meet_ai_lib_sync_auto_TicketSyncState,
  type meet_ai_lib_sync_check_TrackerCheck,
  TICKET_SYNC_EVENT,
} from "./bindings";
import { call, hasBackend, narrow, subscribe } from "./client";
import type { TicketSummary, Tracker, TrackerServer, TrackerSettings, UiError } from "./types";

export { TICKET_SYNC_EVENT };

// --- tickets --------------------------------------------------------------

export async function listTickets(): Promise<TicketSummary[]> {
  if (!hasBackend()) return [];
  return narrow(() => commands.listTickets());
}

export function createTicket(title: string, body: string): Promise<TicketSummary> {
  return narrow(() => commands.createTicket(title, body));
}

/**
 * The Start Work prompt for one ticket (L14), rendered by Rust from the user's
 * template. The window only copies it: work starts in the user's own agent
 * session in their repo, which the app does not run. `meetingId` is the
 * meeting the ticket came from, or null for one made by hand.
 */
export function startWorkPrompt(ticketId: string, meetingId: string | null): Promise<string> {
  return call(() => commands.startWorkPrompt(ticketId, meetingId));
}

// --- a meeting's suggested tasks (TUR-113) ----------------------------------

/**
 * The tasks that came out of one meeting: the suggestions in its own
 * `tickets/` folder (`suggested: true`), plus the approved ones in Tickets
 * that name it. Sorted by id.
 */
export async function meetingTasks(meetingId: string): Promise<TicketSummary[]> {
  if (!hasBackend()) return [];
  return narrow(() => commands.meetingTasks(meetingId));
}

/** Approve one suggested task: it moves to Tickets. Answers with it as Tickets lists it. */
export function approveTask(meetingId: string, ticketId: string): Promise<TicketSummary> {
  return narrow(() => commands.approveTask(meetingId, ticketId));
}

/** Approve every task of the meeting that is still a suggestion. Answers with all its tasks. */
export function approveAllTasks(meetingId: string): Promise<TicketSummary[]> {
  return narrow(() => commands.approveAllTasks(meetingId));
}

/** Discard one suggested task. Its number is never handed out again. */
export async function discardTask(meetingId: string, ticketId: string): Promise<void> {
  await call(() => commands.discardTask(meetingId, ticketId));
}

// --- sending to the tracker (TUR-11, TUR-113) -------------------------------

/** How far one ticket is in being sent on its own. */
export type TicketSyncState = meet_ai_lib_sync_auto_TicketSyncState;

/** One ticket's send state, as {@link TICKET_SYNC_EVENT} carries it. */
export type TicketSyncStatus = {
  ticketId: string;
  state: TicketSyncState;
  /** Why it was not sent. Only when `state` is `failed`. */
  error: UiError | null;
  /** The ticket with its issue key. Only when `state` is `sent`. */
  ticket: TicketSummary | null;
};

/** Whether a tracker is set up, which one, and every ticket queued, sending or failed. */
export type TicketSyncOverview = {
  trackerSetUp: boolean;
  tracker: Tracker;
  tickets: TicketSyncStatus[];
};

/** What "Send a test ticket" found, in plain words. Nothing is created. */
export type TrackerCheck = meet_ai_lib_sync_check_TrackerCheck;

const NO_SYNC: TicketSyncOverview = { trackerSetUp: false, tracker: "linear", tickets: [] };

export async function ticketSyncStates(): Promise<TicketSyncOverview> {
  if (!hasBackend()) return NO_SYNC;
  return narrow(() => commands.ticketSyncStates());
}

/** Send a failed ticket again. Returns at once; the result comes on {@link onTicketSync}. */
export async function retryTicketSync(ticketId: string): Promise<void> {
  await call(() => commands.retryTicketSync(ticketId));
}

/** Every change in one ticket's send state. Returns the unsubscribe function. */
export function onTicketSync(onStatus: (status: TicketSyncStatus) => void): () => void {
  return subscribe<TicketSyncStatus>(TICKET_SYNC_EVENT, onStatus);
}

/** Check that the agent reaches `tracker` through `trackerMcp` (saved or not). Creates nothing. */
export function sendTestTicket(tracker: Tracker, trackerMcp: string): Promise<TrackerCheck> {
  return call(() => commands.sendTestTicket(tracker, trackerMcp));
}

/**
 * Ask the user's agent CLI to create this ticket's issue in their tracker,
 * through the tracker's MCP server. One agent run per ticket, and it can take
 * minutes. Resolves with the ticket as it now reads (`syncedTo`, `externalId`,
 * `externalUrl` set); a run stopped by {@link cancelSync} rejects with kind
 * `agent-cancelled`. `meetingId` is the meeting whose folder holds the ticket,
 * or null for a shared one.
 */
export function syncTask(ticketId: string, meetingId: string | null): Promise<TicketSummary> {
  return narrow(() => commands.syncTask(ticketId, meetingId));
}

/** Stop this ticket's running sync. `meetingId` as for {@link syncTask}. */
export async function cancelSync(ticketId: string, meetingId: string | null): Promise<void> {
  await call(() => commands.cancelSync(ticketId, meetingId));
}

/** Forget the issue a Sync created but could not attach, so the next Sync runs afresh. */
export async function dismissUnsavedSync(
  ticketId: string,
  meetingId: string | null,
): Promise<void> {
  await call(() => commands.dismissUnsavedSync(ticketId, meetingId));
}

/** Open a synced ticket's issue in the browser. Rust opens it; the window never opens URLs. */
export async function openSyncedIssue(ticketId: string, meetingId: string | null): Promise<void> {
  await call(() => commands.openSyncedIssue(ticketId, meetingId));
}

/** What Sync uses when there is no Rust side to ask: the shipped defaults. */
const DEFAULT_TRACKER_SETTINGS: TrackerSettings = {
  tracker: "linear",
  trackerMcp: "claude.ai Linear",
  harness: "claude-code",
  chosen: false,
};

export async function trackerSettings(): Promise<TrackerSettings> {
  if (!hasBackend()) return DEFAULT_TRACKER_SETTINGS;
  return narrow(() => commands.trackerSettings());
}

export function setTracker(tracker: Tracker, trackerMcp: string): Promise<TrackerSettings> {
  return narrow(() => commands.setTracker(tracker, trackerMcp));
}

/**
 * The MCP servers the agent knows about, from `claude mcp list` or
 * `codex mcp list --json`. That can take up to a minute, so never hold a
 * screen on it.
 */
export async function trackerServers(): Promise<TrackerServer[]> {
  if (!hasBackend()) return [];
  return narrow(() => commands.trackerServers());
}
