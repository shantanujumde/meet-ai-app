/**
 * The meeting list's content pane — what fills the space when no meeting is
 * open.
 *
 * Three real states, and the empty one is not a consolation prize: before the
 * first recording it is the *only* screen a new user sees after onboarding, so
 * it has to say what this app is for and how to start.
 */

import { useState } from "react";
import { useNavigate } from "react-router";
import { cn } from "@/lib/cn";
import { DEFAULT_ROOT_LABEL, SHORTCUT_LABEL } from "@/lib/constants";
import {
  formatLineCount,
  formatRelativeDate,
  INTERRUPTED_LABEL,
  NOTES_OFF_LABEL,
} from "@/lib/format";
import { openPermissionScreen } from "@/lib/permissionRoute";
import { meetingPath } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import {
  Button,
  ButtonRow,
  cardVariants,
  Pill,
  RowLabel,
  RowValue,
  rowVariants,
} from "@/ui/primitives";
import { SearchBox } from "@/ui/SearchBox";
import { SearchResults } from "@/ui/SearchResults";
import { Checking, EmptyState, ErrorState } from "@/ui/states";
import { TodayPane } from "@/ui/TodayPane";

export function Meetings() {
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
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
        <TodayPane />
        <EmptyState
          title="No meetings yet"
          body={
            list?.rootExists
              ? `Press ${SHORTCUT_LABEL} — from anywhere, even with this window behind Zoom — and meet-ai starts recording. Everything it captures is written as plain markdown into ${list.root}, and nothing leaves this Mac.`
              : `Press ${SHORTCUT_LABEL} — from anywhere, even with this window behind Zoom — and meet-ai starts recording. It will create ${list?.root ?? DEFAULT_ROOT_LABEL} for the first one. Everything is plain markdown, and nothing leaves this Mac.`
          }
          action={
            <ButtonRow>
              <Button
                tone="primary"
                disabled={permission?.state === "denied" || recordingBusy}
                onClick={() => void toggle()}
              >
                Start recording
              </Button>
              {permission?.state === "denied" ? (
                <Button onClick={() => openPermissionScreen(navigate)}>
                  Fix audio permission first
                </Button>
              ) : null}
            </ButtonRow>
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

      <TodayPane />

      <SearchBox value={query} onChange={setQuery} />

      {query.trim() ? (
        <SearchResults query={query} onOpen={(id) => navigate(meetingPath(id))} />
      ) : (
        <section className={cardVariants({ flush: true })}>
          {meetings.map((meeting) => (
            // The whole row is the button. It draws no hairline between rows —
            // the list reads as one block, and each row is its own hit target.
            <button
              key={meeting.id}
              type="button"
              className={cn(rowVariants({ divided: false }), "w-full")}
              onClick={() => navigate(meetingPath(meeting.id))}
            >
              <RowLabel
                name={meeting.title}
                detail={`${formatRelativeDate(meeting.date)}${meeting.time ? ` at ${meeting.time}` : ""}`}
                mono={false}
              />
              <RowValue>
                {meeting.recordingState === "interrupted" ? (
                  <Pill tone="warn" className="me-4">
                    {INTERRUPTED_LABEL}
                  </Pill>
                ) : null}
                {/* TUR-12: this meeting is never sent to an agent. */}
                {meeting.notesOff ? <Pill className="me-4">{NOTES_OFF_LABEL}</Pill> : null}
                {formatLineCount(meeting.lineCount)}
                {meeting.hasAnalysis ? " · wrapped up" : ""}
              </RowValue>
            </button>
          ))}
        </section>
      )}
    </div>
  );
}
