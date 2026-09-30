/**
 * The meeting list.
 *
 * Native window vibrancy is already behind this surface, so it adds no blur of
 * its own — stacking a `backdrop-filter` on top of the window material is the
 * "glass on glass" the design system rules out.
 *
 * Selection is marked by a fill *and* a leading accent bar, never by colour
 * alone.
 */

import { NavLink, useNavigate } from "react-router";
import type { MeetingList, MeetingSummary, RecordingStatus } from "@/ipc/types";
import { formatLineCount, formatRelativeDate, INTERRUPTED_LABEL } from "./format";
import { Checking } from "./states";

export function Sidebar({
  list,
  loading,
  recording,
  selectedId,
}: {
  list: MeetingList | null;
  loading: boolean;
  recording: RecordingStatus;
  selectedId: string | undefined;
}) {
  const navigate = useNavigate();
  const meetings = list?.meetings ?? [];

  return (
    <nav className="sidebar" aria-label="Meetings">
      <div className="sidebar__heading">
        <span>Meetings</span>
        {!loading && meetings.length > 0 ? (
          <span className="sidebar__count">{meetings.length}</span>
        ) : null}
      </div>

      {loading ? (
        <div style={{ padding: "var(--space-4) var(--space-3)" }}>
          <Checking label="Reading your meetings folder…" />
        </div>
      ) : meetings.length === 0 ? (
        // The sidebar's empty state is one quiet line. The full explanation
        // belongs on the content pane beside it, not shouted twice.
        <p
          style={{
            margin: 0,
            padding: "var(--space-4) var(--space-3)",
            fontSize: "var(--text-caption1)",
            color: "var(--text-tertiary)",
            lineHeight: "var(--leading-normal)",
          }}
        >
          Nothing recorded yet.
        </p>
      ) : (
        <ul className="sidebar__list">
          {meetings.map((meeting) => (
            <li key={meeting.id}>
              <MeetingRow
                meeting={meeting}
                selected={meeting.id === selectedId}
                isRecording={recording.meetingId === meeting.id}
                onOpen={() => navigate(`/meetings/${encodeURIComponent(meeting.id)}`)}
              />
            </li>
          ))}
        </ul>
      )}

      <div className="sidebar__footer">
        <NavLink to="/settings" className="btn btn--quiet btn--small btn--block">
          Settings
        </NavLink>
      </div>
    </nav>
  );
}

function MeetingRow({
  meeting,
  selected,
  isRecording,
  onOpen,
}: {
  meeting: MeetingSummary;
  selected: boolean;
  isRecording: boolean;
  onOpen: () => void;
}) {
  return (
    <button type="button" className="meeting-row" aria-current={selected} onClick={onOpen}>
      <span className="meeting-row__title">{meeting.title}</span>
      <span className="meeting-row__meta">
        <span>{formatRelativeDate(meeting.date)}</span>
        {meeting.time ? <span>{meeting.time}</span> : null}
        {/* A recording meeting shows the dot, not a red title — colour alone
            is not a state signal. */}
        {isRecording ? (
          <span className="meeting-row__badge" style={{ color: "var(--status-recording)" }}>
            ● Recording
          </span>
        ) : meeting.recordingState === "interrupted" ? (
          // Takes the line count's place: the sidebar is too narrow for both,
          // and the meeting itself says how much was kept.
          <span className="meeting-row__badge meeting-row__badge--interrupted">
            {INTERRUPTED_LABEL}
          </span>
        ) : (
          <span className="meeting-row__badge">{formatLineCount(meeting.lineCount)}</span>
        )}
      </span>
    </button>
  );
}
