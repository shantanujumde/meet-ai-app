/**
 * Reviewing a finished meeting: its transcript and its notes.
 *
 * The transcript is **read-only here, always**. L7 makes `transcript.md` the
 * source of truth and §3.4 makes it append-only, so this screen renders it and
 * never offers to edit it. The notes pane beside it is the writable half.
 *
 * The live transcript pane is not this screen. That is Phase 2b and needs the
 * streaming session API; what is here reads a finished file off disk.
 */

import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { readMeeting, revealMeeting } from "@/ipc/client";
import type { MeetingDetail, TranscriptLine, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { useAppStore } from "@/state/app";
import { describeInterruption, formatRelativeDate, INTERRUPTED_LABEL } from "@/ui/format";
import { NotesPane } from "@/ui/NotesPane";
import { Checking, EmptyState, ErrorState } from "@/ui/states";

export function Review() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const reloadMeetings = useAppStore((state) => state.loadMeetings);

  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async (meetingId: string) => {
    setLoading(true);
    try {
      setDetail(await readMeeting(meetingId));
      setError(null);
    } catch (thrown) {
      setError(toUiError(thrown));
      setDetail(null);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!id) return;
    void load(id);
  }, [id, load]);

  if (!id) {
    return (
      <div className="page">
        <EmptyState title="No meeting selected" body="Pick one from the list on the left." />
      </div>
    );
  }

  if (loading && detail === null) {
    return (
      <div className="page">
        <Checking label="Opening this meeting…" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="page">
        <ErrorState
          error={error}
          onRemedy={() => {
            void reloadMeetings();
            navigate("/meetings");
          }}
        />
      </div>
    );
  }

  if (!detail) return null;

  const { summary, lines, transcriptMissing, unparsedLineCount, path } = detail;
  const interrupted = summary.recordingState === "interrupted";

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="page__title">{summary.title}</h1>
        <p className="page__meta">
          <span>{formatRelativeDate(summary.date)}</span>
          {summary.time ? <span>{summary.time}</span> : null}
          {summary.lastTimestamp ? <span>Last line at {summary.lastTimestamp}</span> : null}
          {summary.hasAnalysis ? <span>Wrapped up</span> : null}
          {interrupted ? <span>{INTERRUPTED_LABEL}</span> : null}
        </p>
        {/* TUR-97: an interrupted meeting opens like any other — everything
            below still renders — but says in one line that it did not end on
            purpose, and how much of it was kept. */}
        {interrupted ? (
          <p className="page__notice">{describeInterruption(summary.audioMs)}</p>
        ) : null}
        <div className="btn-row">
          <button
            type="button"
            className="btn btn--small"
            onClick={() => void revealMeeting(summary.id)}
          >
            Show in Finder
          </button>
          <span className="row__detail">{path}</span>
        </div>
      </header>

      <section className="section" aria-labelledby="transcript-heading">
        <div className="section__header">
          <h2 className="section__title" id="transcript-heading">
            Transcript
          </h2>
          <p className="section__hint">Read-only — transcript.md is the record</p>
        </div>

        {/* SPEC §7: the UI flags a file it could only partly read rather than
            failing, and rather than pretending it read all of it. */}
        {unparsedLineCount > 0 ? (
          <p className="state__detail">
            {unparsedLineCount === 1
              ? "1 line in this file is not in meet-ai's transcript format and is not shown below. Nothing has been changed — open the file in Finder to see it."
              : `${unparsedLineCount} lines in this file are not in meet-ai's transcript format and are not shown below. Nothing has been changed — open the file in Finder to see them.`}
          </p>
        ) : null}

        {transcriptMissing ? (
          <EmptyState
            title="There is no transcript file for this meeting"
            body="The folder exists but transcript.md is not in it. That happens if the file was moved or deleted outside meet-ai — your notes below are unaffected."
          />
        ) : lines.length === 0 ? (
          <EmptyState
            title="Nothing was transcribed"
            body="transcript.md is empty. Either nobody spoke, or this meeting was recorded before transcription was switched on. The audio, if it was kept, is still in the meeting folder."
          />
        ) : (
          <ol className="transcript">
            {lines.map((line) => (
              <TranscriptRow key={line.seq} line={line} />
            ))}
          </ol>
        )}
      </section>

      <NotesPane
        meetingId={summary.id}
        initialNotes={detail.notes}
        // A meeting that gains notes changes how it reads in the list.
        onSaved={() => void reloadMeetings()}
      />
    </div>
  );
}

function TranscriptRow({ line }: { line: TranscriptLine }) {
  return (
    <li className="transcript__line">
      <time className="transcript__time">{line.time}</time>
      {/* Colour paired with an initial, so the speakers stay distinguishable
          in greyscale and to a colourblind reader. The full label is on the
          element for VoiceOver. */}
      <span className="transcript__speaker" data-speaker={line.speaker} aria-hidden="true">
        {line.speaker === "You" ? "Y" : "O"}
      </span>
      <span className="transcript__text">
        <span className="sr-only">{line.speaker}: </span>
        {line.text}
      </span>
    </li>
  );
}
