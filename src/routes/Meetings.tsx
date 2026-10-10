/**
 * The meeting list's content pane — what fills the space when no meeting is
 * open.
 *
 * Three real states, and the empty one is not a consolation prize: before the
 * first recording it is the *only* screen a new user sees after onboarding, so
 * it has to say what this app is for and how to start.
 */

import { AudioLines, FileText } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import type { MeetingSummary } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { DEFAULT_ROOT_LABEL } from "@/lib/constants";
import {
  formatLineCount,
  formatRelativeDate,
  INTERRUPTED_LABEL,
  NOTES_OFF_LABEL,
} from "@/lib/format";
import { openPermissionScreen } from "@/lib/permissionRoute";
import { recordDisabled } from "@/lib/recordingPermission";
import { meetingPath } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { MeetingRowMenu, RenameField } from "@/ui/MeetingRowMenu";
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
import { useRecordShortcut } from "@/ui/useRecordShortcut";
import { WatchProblemNote } from "@/ui/WatchProblemNote";

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
  const recording = useRecordingStore((state) => state.status);
  // TUR-169: a shortcut another app owns is not advertised.
  const shortcut = useRecordShortcut();
  const howToStart = shortcut.available
    ? `Press ${shortcut.label} from anywhere, even with this window behind Zoom, and meet-ai starts recording.`
    : `Click Start recording and meet-ai starts recording. ${shortcut.label} is unavailable because another app is using it.`;

  const meetings = list?.meetings ?? [];
  // Which of the three states shows. The list only once it has meetings.
  const state =
    loading && list === null
      ? "loading"
      : error
        ? "error"
        : meetings.length === 0
          ? "empty"
          : "list";

  let body: ReactNode;
  if (state === "loading") {
    body = <Checking label="Reading your meetings folder…" />;
  } else if (error) {
    body = <ErrorState error={error} onRemedy={() => void reload()} />;
  } else if (state === "empty") {
    body = (
      <EmptyState
        title="No meetings yet"
        body={
          list?.rootExists
            ? `${howToStart} Everything is saved as plain text files in ${list.root}, and nothing leaves this Mac.`
            : `${howToStart} It creates ${list?.root ?? DEFAULT_ROOT_LABEL} for the first one. Everything is saved as plain text files, and nothing leaves this Mac.`
        }
        action={
          <ButtonRow>
            <Button
              tone="primary"
              disabled={recordDisabled(recording, permission, recordingBusy)}
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
    );
  } else {
    body = (
      <>
        <SearchBox value={query} onChange={setQuery} />

        {query.trim() ? (
          <SearchResults query={query} onOpen={(id) => navigate(meetingPath(id))} />
        ) : (
          <section className={cardVariants({ flush: true })}>
            {meetings.map((meeting) => (
              <MeetingListRow
                key={meeting.id}
                meeting={meeting}
                isRecording={recording.phase !== "idle" && recording.meetingId === meeting.id}
                onOpen={() => navigate(meetingPath(meeting.id))}
              />
            ))}
          </section>
        )}
      </>
    );
  }

  // One TodayPane at one place in the tree for every state (TUR-171): a pane
  // per state was a new one each time the state changed, so every launch
  // read the calendar twice and showed "Reading your calendar…" twice.
  return (
    <div className={cn("page", state === "empty" && "page--narrow")}>
      {state === "list" ? (
        <header className="page__header">
          <h1 className="page__title">Meetings</h1>
          <p className="page__meta">
            <span>{meetings.length === 1 ? "1 meeting" : `${meetings.length} meetings`}</span>
            <span>{list?.root}</span>
          </p>
        </header>
      ) : null}
      {state === "list" ? <WatchProblemNote /> : null}

      <TodayPane />

      {body}
    </div>
  );
}

/**
 * One meeting in the list. The whole row is the button. It draws no hairline
 * between rows: the list reads as one block, and each row is its own hit
 * target. Its ⋯ menu sits beside the button at the row's end, and the same
 * menu opens on a right-click (TUR-116).
 */
function MeetingListRow({
  meeting,
  isRecording,
  onOpen,
}: {
  meeting: MeetingSummary;
  isRecording: boolean;
  onOpen: () => void;
}) {
  // Rename… turns the name into a text field in place. Closing it from the
  // keyboard puts focus back on the row.
  const [renaming, setRenaming] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const refocus = useRef(false);
  useEffect(() => {
    if (!renaming && refocus.current) {
      refocus.current = false;
      button.current?.focus();
    }
  }, [renaming]);

  const label = (
    <RowLabel
      icon={meeting.recordingState === "interrupted" ? AudioLines : FileText}
      name={
        <>
          {renaming ? (
            <RenameField
              meeting={meeting}
              onDone={(again) => {
                refocus.current = again;
                setRenaming(false);
              }}
            />
          ) : (
            meeting.title
          )}
          {/* TUR-29: who was invited, from the calendar event. */}
          {meeting.attendees.length > 0 ? (
            <span className="block text-footnote font-normal text-fg-secondary">
              {meeting.attendees.join(", ")}
            </span>
          ) : null}
        </>
      }
      detail={`${formatRelativeDate(meeting.date)}${meeting.time ? ` at ${meeting.time}` : ""}`}
      mono={false}
    />
  );
  const value = (
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
  );
  const rowClass = cn(rowVariants({ divided: false }), "w-full text-start pe-9");

  return (
    <MeetingRowMenu
      meeting={meeting}
      isRecording={isRecording}
      selected={false}
      renaming={renaming}
      onRename={() => setRenaming(true)}
    >
      {renaming ? (
        <div className={rowClass}>
          {label}
          {value}
        </div>
      ) : (
        <button ref={button} type="button" className={rowClass} onClick={onOpen}>
          {label}
          {value}
        </button>
      )}
    </MeetingRowMenu>
  );
}
