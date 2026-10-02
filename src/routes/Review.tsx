/**
 * Reviewing a finished meeting: its transcript and its notes.
 *
 * The transcript is **read-only here, always**. L7 makes `transcript.md` the
 * source of truth and §3.4 makes it append-only, so this screen renders it and
 * never offers to edit it. The notes pane beside it is the writable half.
 *
 * While this meeting is the one recording, the file-backed transcript gives way
 * to the live pane (TUR-96): the file only has settled lines and would sit
 * there looking stale. Once the recording ends the meeting is re-read, so what
 * shows next is the finished `transcript.md`.
 */

import { memo, useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { copyPromptFallback, readMeeting, revealMeeting, wrapUpPrompt } from "@/ipc/client";
import type { MeetingDetail, TranscriptLine, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { showsCopyPrompt } from "@/lib/copyPrompt";
import { describeInterruption, formatRelativeDate, INTERRUPTED_LABEL } from "@/lib/format";
import { MEETINGS } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { useTranscriptStore } from "@/state/transcript";
import { CopyPromptButton } from "@/ui/CopyPromptButton";
import { LiveTranscript } from "@/ui/LiveTranscript";
import { NotesPane } from "@/ui/NotesPane";
import { Button, ButtonRow, rowDetailVariants } from "@/ui/primitives";
import { SpeakerLabel } from "@/ui/SpeakerLabel";
import { Checking, EmptyState, ErrorState } from "@/ui/states";

export function Review() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const reloadMeetings = useAppStore((state) => state.loadMeetings);

  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [loading, setLoading] = useState(true);
  // Finder failing to open the folder is its own, smaller problem: the
  // meeting is still readable, so it gets a message under the button rather
  // than replacing the screen.
  const [revealError, setRevealError] = useState<UiError | null>(null);

  const reveal = useCallback(async (meetingId: string) => {
    setRevealError(null);
    try {
      await revealMeeting(meetingId);
    } catch (thrown) {
      setRevealError(toUiError(thrown));
    }
  }, []);

  // A11's fallback: with no agent to run, a meeting is wrapped up by copying
  // its prompt. Asked once; a failed answer counts as "no", since the button
  // is an extra and the meeting reads fine without it.
  const [harnessIsNone, setHarnessIsNone] = useState(false);
  useEffect(() => {
    let current = true;
    copyPromptFallback().then(
      (answer) => {
        if (current) setHarnessIsNone(answer);
      },
      () => {},
    );
    return () => {
      current = false;
    };
  }, []);
  // `cliFound` stays at its default until agent CLI detection (TUR-6, TUR-10)
  // can say whether the chosen agent is installed.
  const copyPrompt = showsCopyPrompt({ harnessIsNone });

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

  const recording = useRecordingStore((state) => state.status);
  const live = useTranscriptStore((state) => state.live);
  const isLive = id !== undefined && recording.meetingId === id && recording.phase !== "idle";

  // When this meeting's recording ends, the file on disk is now the finished
  // record — re-read it, or the screen shows whatever was there at the start.
  // A store subscription for the same reason as in App's Bootstrap: the phase
  // is a trigger here, not something this effect displays.
  useEffect(() => {
    if (!id) return;
    return useRecordingStore.subscribe((state, previous) => {
      const ended = previous.status.phase !== "idle" && state.status.phase === "idle";
      if (ended && previous.status.meetingId === id) void load(id);
    });
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
            navigate(MEETINGS);
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
        <ButtonRow>
          <Button size="small" onClick={() => void reveal(summary.id)}>
            Show in Finder
          </Button>
          <span className={rowDetailVariants()}>{path}</span>
        </ButtonRow>
        {revealError ? <ErrorState error={revealError} /> : null}
        {/* Not while recording: the transcript is not finished, so the
            prompt would wrap up half a meeting. */}
        {copyPrompt && !isLive ? (
          <CopyPromptButton
            label="Copy prompt"
            size="small"
            hint="No agent set up — paste this into Claude Code or Codex and it will write the notes."
            render={() => wrapUpPrompt(summary.id)}
          />
        ) : null}
      </header>

      {isLive ? (
        <LiveTranscript live={live} />
      ) : (
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
      )}

      <NotesPane
        meetingId={summary.id}
        initialNotes={detail.notes}
        // A meeting that gains notes changes how it reads in the list.
        onSaved={() => void reloadMeetings()}
      />
    </div>
  );
}

/**
 * One line of the finished transcript. Memoised: a two-hour meeting is
 * thousands of rows, and this screen re-renders on every live-transcript
 * event — including while a *different* meeting records — so without it each
 * event re-rendered every line of this one.
 */
const TranscriptRow = memo(function TranscriptRow({ line }: { line: TranscriptLine }) {
  return (
    <li className="transcript__line">
      <time className="transcript__time">{line.time}</time>
      <SpeakerLabel speaker={line.speaker} />
      <span className="transcript__text">
        <span className="sr-only">{line.speaker}: </span>
        {line.text}
      </span>
    </li>
  );
});
