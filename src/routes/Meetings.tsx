/**
 * The meeting list's content pane — what fills the space when no meeting is
 * open.
 *
 * Three real states, and the empty one is not a consolation prize: before the
 * first recording it is the *only* screen a new user sees after onboarding, so
 * it has to say what this app is for and how to start.
 */

import { useNavigate } from "react-router";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { formatLineCount, formatRelativeDate } from "@/ui/format";
import { Checking, EmptyState, ErrorState } from "@/ui/states";

export function Meetings() {
  const navigate = useNavigate();
  const list = useAppStore((state) => state.meetings);
  const loading = useAppStore((state) => state.meetingsLoading);
  const error = useAppStore((state) => state.meetingsError);
  const reload = useAppStore((state) => state.loadMeetings);

  const permission = useAppStore((state) => state.permission);
  const toggle = useRecordingStore((state) => state.toggle);
  const recordingBusy = useRecordingStore((state) => state.busy);

  if (loading && list === null) {
    return (
      <div className="page">
        <Checking label="Reading your meetings folder…" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="page">
        <ErrorState error={error} onRemedy={() => void reload()} />
      </div>
    );
  }

  const meetings = list?.meetings ?? [];

  if (meetings.length === 0) {
    return (
      <div className="page page--narrow">
        <EmptyState
          title="No meetings yet"
          body={
            list?.rootExists
              ? `Press ⌘⇧R — from anywhere, even with this window behind Zoom — and meet-ai starts recording. Everything it captures is written as plain markdown into ${list.root}, and nothing leaves this Mac.`
              : `Press ⌘⇧R — from anywhere, even with this window behind Zoom — and meet-ai starts recording. It will create ${list?.root ?? "~/Meetings"} for the first one. Everything is plain markdown, and nothing leaves this Mac.`
          }
          action={
            <div className="btn-row">
              <button
                type="button"
                className="btn btn--primary"
                disabled={permission?.state === "denied" || recordingBusy}
                onClick={() => void toggle()}
              >
                Start recording
              </button>
              {permission?.state === "denied" ? (
                <button
                  type="button"
                  className="btn"
                  onClick={() => navigate("/onboarding/permission")}
                >
                  Fix audio permission first
                </button>
              ) : null}
            </div>
          }
        />
      </div>
    );
  }

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="page__title">Meetings</h1>
        <p className="page__meta">
          <span>{meetings.length === 1 ? "1 meeting" : `${meetings.length} meetings`}</span>
          <span>{list?.root}</span>
        </p>
      </header>

      <section className="card card--flush">
        {meetings.map((meeting) => (
          <button
            key={meeting.id}
            type="button"
            className="row"
            style={{
              width: "100%",
              border: 0,
              background: "transparent",
              font: "inherit",
              cursor: "default",
            }}
            onClick={() => navigate(`/meetings/${encodeURIComponent(meeting.id)}`)}
          >
            <span className="row__label">
              <span className="row__name">{meeting.title}</span>
              <span className="row__detail" style={{ fontFamily: "var(--font-ui)" }}>
                {formatRelativeDate(meeting.date)}
                {meeting.time ? ` at ${meeting.time}` : ""}
              </span>
            </span>
            <span className="row__value">
              {formatLineCount(meeting.lineCount)}
              {meeting.hasAnalysis ? " · wrapped up" : ""}
            </span>
          </button>
        ))}
      </section>
    </div>
  );
}
