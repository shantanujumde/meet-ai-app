/**
 * Builders for the IPC shapes tests hand to the mocked backend.
 *
 * Each returns a complete, ordinary value — a finished meeting, a read
 * transcript — and takes overrides for the one or two fields a test is
 * actually about. So a test states what is special about its meeting, and a
 * new required field on the Rust side is added here once, not in every file.
 */

import type { MeetingDetail, MeetingSummary, TicketSummary, TranscriptLine } from "@/ipc/types";

/** An open, hand-made ticket that has not been synced to a tracker. */
export function ticketSummary(overrides: Partial<TicketSummary> = {}): TicketSummary {
  return {
    id: "TUR-7",
    title: "Write the docs",
    status: "open",
    meeting: null,
    body: "Cover the setup.",
    hasProblems: false,
    syncedTo: null,
    externalId: null,
    externalUrl: null,
    ...overrides,
  };
}

/** A finished, ordinary meeting with one line and no notes. */
export function meetingSummary(overrides: Partial<MeetingSummary> = {}): MeetingSummary {
  return {
    id: "2026-09-30-1015-meeting",
    title: "Meeting",
    date: "2026-09-30",
    time: "10:15",
    lineCount: 1,
    lastTimestamp: null,
    hasNotes: false,
    hasAnalysis: false,
    recordingState: "finished",
    audioMs: null,
    ...overrides,
  };
}

/** One settled transcript line, as `read_meeting` returns it. */
export function transcriptLine(overrides: Partial<TranscriptLine> = {}): TranscriptLine {
  return {
    seq: 0,
    time: "00:00:04",
    speaker: "You",
    text: "Can everyone hear me?",
    ...overrides,
  };
}

/**
 * A meeting as the review screen reads it. `lineCount` and `path` follow the
 * summary and lines given, so the two cannot contradict each other.
 */
export function meetingDetail(
  overrides: Partial<Omit<MeetingDetail, "summary">> & { summary?: Partial<MeetingSummary> } = {},
): MeetingDetail {
  const { summary: summaryOverrides, ...rest } = overrides;
  const lines = rest.lines ?? [];
  const summary = meetingSummary({ lineCount: lines.length, ...summaryOverrides });
  return {
    path: `/Users/test/Meetings/${summary.id}`,
    transcriptMissing: false,
    unparsedLineCount: 0,
    notes: "",
    ...rest,
    summary,
    lines,
  };
}
