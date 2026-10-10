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
 *
 * The header is the title, one meta line and the folder actions (TUR-81).
 * Renaming the meeting (TUR-103) updates the header and the list at once. A
 * notes run that names it, or the calendar naming it while the page is open
 * (TUR-107), re-reads only the header's summary, never the notes, so nothing
 * the user is typing is replaced.
 * The "Make notes for this meeting" switch (TUR-12, SPEC A11) heads the
 * meeting-notes section, recording or not, so a private call can be switched
 * off before it ends. One {@link useNotesRun} serves the switch and the run.
 */

import { memo, useCallback, useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { useCliFound } from "@/hooks/useCliFound";
import { useNotesRun } from "@/hooks/useNotesRun";
import {
  copyPromptFallback,
  readMeeting,
  renameMeeting,
  revealMeeting,
  wrapUpPrompt,
} from "@/ipc/client";
import type { MeetingDetail, MeetingList, TranscriptLine, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { showsCopyPrompt } from "@/lib/copyPrompt";
import { osText } from "@/lib/osText";
import { MEETINGS } from "@/lib/routes";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { useTranscriptStore } from "@/state/transcript";
import { CopyPromptButton } from "@/ui/CopyPromptButton";
import { HookFailedNote } from "@/ui/HookFailedNote";
import { LiveTranscript } from "@/ui/LiveTranscript";
import { MeetingHeader } from "@/ui/MeetingHeader";
import { MeetingTasks } from "@/ui/MeetingTasks";
import { NotesPane } from "@/ui/NotesPane";
import { NotesRun } from "@/ui/NotesRun";
import { NotesSwitch } from "@/ui/NotesSwitch";
import { Card, Row } from "@/ui/primitives";
import { SpeakerLabel } from "@/ui/SpeakerLabel";
import { Checking, EmptyState, ErrorState } from "@/ui/states";

export function Review() {
  const { id } = useParams<{ id: string }>();
  if (!id) {
    return (
      <div className="page">
        <EmptyState title="No meeting selected" body="Pick one from the list on the left." />
      </div>
    );
  }
  // Keyed on the id (TUR-150): another meeting is a fresh screen, so nothing
  // of the last one (its detail, a read still out) can show under this URL.
  return <MeetingReview key={id} id={id} />;
}

function MeetingReview({ id }: { id: string }) {
  const navigate = useNavigate();
  const reloadMeetings = useAppStore((state) => state.loadMeetings);

  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [loading, setLoading] = useState(true);
  // Finder failing to open the folder is its own, smaller problem: the
  // meeting is still readable, so it gets a message under the header rather
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
  // Null until asked, so the notes' start button does not flash up before
  // the answer says Copy prompt stands in for it.
  const [harnessIsNone, setHarnessIsNone] = useState<boolean | null>(null);
  useEffect(() => {
    let current = true;
    copyPromptFallback().then(
      (answer) => {
        if (current) setHarnessIsNone(answer);
      },
      () => {
        if (current) setHarnessIsNone(false);
      },
    );
    return () => {
      current = false;
    };
  }, []);
  // With an agent set up, Copy prompt also stands in when its CLI is missing.
  const cliFound = useCliFound(harnessIsNone === false);
  const copyPrompt = showsCopyPrompt({ harnessIsNone: harnessIsNone === true, cliFound });

  // Bumped by every read, so only the newest one's answer is shown: an older
  // read landing last must not put stale data back (TUR-150).
  const reads = useRef(0);
  const load = useCallback(async (meetingId: string) => {
    reads.current += 1;
    const read = reads.current;
    setLoading(true);
    try {
      const answer = await readMeeting(meetingId);
      if (read !== reads.current) return;
      setDetail(answer);
      setError(null);
    } catch (thrown) {
      if (read !== reads.current) return;
      setError(toUiError(thrown));
      setDetail(null);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load(id);
  }, [id, load]);

  // Bumped by every rename, so a summary read that started before one does
  // not land after it and put the agent's title back over the user's.
  const renames = useRef(0);
  // Renames still waiting for their answer. A read started while one is out
  // could land after it with the old title, so none starts then (TUR-107).
  const renamesOut = useRef(0);

  // Only the summary: replacing the whole detail would hand the notes pane
  // what is on disk while the user may still be typing.
  const refreshSummary = useCallback(async (meetingId: string) => {
    if (renamesOut.current > 0) return;
    const before = renames.current;
    let summary: MeetingDetail["summary"];
    try {
      summary = (await readMeeting(meetingId)).summary;
    } catch {
      return; // The header keeps what it had.
    }
    if (renames.current !== before) return;
    setDetail((current) => (current?.summary.id === meetingId ? { ...current, summary } : current));
  }, []);

  const rename = useCallback(
    async (title: string) => {
      renames.current += 1;
      renamesOut.current += 1;
      let saved: string;
      try {
        saved = await renameMeeting(id, title);
      } finally {
        renamesOut.current -= 1;
      }
      setDetail((current) =>
        current?.summary.id === id
          ? { ...current, summary: { ...current.summary, title: saved } }
          : current,
      );
      void reloadMeetings({ silent: true });
    },
    [id, reloadMeetings],
  );

  // The calendar can name this meeting while its page is open (TUR-107). The
  // list learns of it from the folder watcher; the header follows when the
  // list's title for this meeting changes and no longer matches the header.
  // It re-reads the summary rather than copying the list's title, so the
  // `renames` guards keep a rename in flight from being put back.
  const shownTitle = useRef<string | undefined>(undefined);
  useEffect(() => {
    shownTitle.current = detail?.summary.title;
  }, [detail]);
  useEffect(() => {
    return useAppStore.subscribe((state, previous) => {
      const listed = listedTitle(state.meetings, id);
      if (listed === undefined || listed === listedTitle(previous.meetings, id)) return;
      if (shownTitle.current !== undefined && listed !== shownTitle.current) {
        void refreshSummary(id);
      }
    });
  }, [id, refreshSummary]);

  const recording = useRecordingStore((state) => state.status);
  const live = useTranscriptStore((state) => state.live);
  const isLive = recording.meetingId === id && recording.phase !== "idle";

  // A run writing notes and the switch moving both change how this meeting
  // reads in the list (its "Notes off" marker, say), and a run may give it
  // the agent's title (TUR-103).
  const notesRun = useNotesRun(id, () => {
    void reloadMeetings({ silent: true });
    void refreshSummary(id);
  });

  // When this meeting's recording ends, the file on disk is now the finished
  // record — re-read it, or the screen shows whatever was there at the start.
  // A store subscription for the same reason as in App's Bootstrap: the phase
  // is a trigger here, not something this effect displays.
  useEffect(() => {
    return useRecordingStore.subscribe((state, previous) => {
      const ended = previous.status.phase !== "idle" && state.status.phase === "idle";
      if (ended && previous.status.meetingId === id) void load(id);
    });
  }, [id, load]);

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
            void reloadMeetings({ silent: true });
            navigate(MEETINGS);
          }}
        />
      </div>
    );
  }

  if (!detail) return null;

  const { summary, lines, transcriptMissing, unparsedLineCount, path } = detail;
  // The notes as last read are the newest word; the list's summary stands in
  // until they arrive, so a switched-off meeting never flashes on.
  const notesOn = !(notesRun.notes?.notesOff ?? summary.notesOff);

  return (
    <div className="page">
      <MeetingHeader
        summary={summary}
        path={path}
        live={isLive ? recording : null}
        onReveal={() => void reveal(summary.id)}
        onRename={rename}
        revealError={revealError}
      />
      {/* TUR-63: a user hook that failed for this meeting. */}
      <HookFailedNote meetingId={summary.id} />

      {/* The agent's notes (TUR-10), headed by their switch (TUR-12). While
          recording only the switch shows: the run starts on its own when this
          recording stops, and its status event arrives. */}
      <NotesRun
        run={notesRun}
        live={isLive}
        canStart={harnessIsNone !== null && !copyPrompt && lines.length > 0}
        toggle={
          <Card flush>
            <NotesSwitch
              on={notesOn}
              busy={notesRun.switching}
              error={notesRun.switchError}
              onChange={notesRun.setNotesOn}
            />
            {/* Not while recording: the transcript is not finished, so the
                prompt would wrap up half a meeting. Not with notes off
                either: the user has said this one is not for an agent. */}
            {copyPrompt && !isLive && notesOn ? (
              <Row>
                <CopyPromptButton
                  label="Copy prompt"
                  size="small"
                  hint="No agent is set up. Paste this into Claude Code or Codex and it will write the notes."
                  render={() => wrapUpPrompt(summary.id)}
                />
              </Row>
            ) : null}
          </Card>
        }
      />

      {isLive ? (
        <LiveTranscript live={live} />
      ) : (
        <section className="section" aria-labelledby="transcript-heading">
          <div className="section__header">
            <h2 className="section__title" id="transcript-heading">
              Transcript
            </h2>
            <p className="section__hint" title="transcript.md in the meeting folder">
              Read-only
            </p>
          </div>

          {/* SPEC §7: the UI flags a file it could only partly read rather than
              failing, and rather than pretending it read all of it. */}
          {unparsedLineCount > 0 ? (
            <p className="state__detail">
              {unparsedLineCount === 1
                ? `1 line in this file is not in meet-ai's transcript format and is not shown below. Nothing was changed. Open the file in ${osText("fileManager")} to see it.`
                : `${unparsedLineCount} lines in this file are not in meet-ai's transcript format and are not shown below. Nothing was changed. Open the file in ${osText("fileManager")} to see them.`}
            </p>
          ) : null}

          {transcriptMissing ? (
            <EmptyState
              title="There is no transcript file for this meeting"
              body="The folder exists but transcript.md is not in it. That happens if the file was moved or deleted outside meet-ai. Your notes below are not affected."
            />
          ) : lines.length === 0 ? (
            <EmptyState
              title="Nothing was transcribed"
              body="transcript.md is empty. Either nobody spoke, or this meeting was recorded before transcription was turned on. The audio, if it was kept, is still in the meeting folder."
            />
          ) : (
            <Card
              className="relative max-h-[min(24rem,45vh)] overflow-y-auto"
              tabIndex={0}
              role="region"
              aria-label="Transcript"
            >
              <ol className="transcript">
                {lines.map((line, i) => (
                  <TranscriptRow
                    key={line.seq}
                    line={line}
                    showSpeaker={i === 0 || lines[i - 1]?.speaker !== line.speaker}
                  />
                ))}
              </ol>
            </Card>
          )}
        </section>
      )}

      {isLive ? null : <MeetingTasks meetingId={summary.id} />}

      {/* Keyed on the id: the notes on disk are read into a fresh pane, and a
          re-read of this meeting never puts them back over what is typed. */}
      <NotesPane
        key={summary.id}
        meetingId={summary.id}
        initialNotes={detail.notes}
        // A meeting that gains notes changes how it reads in the list.
        onSaved={() => void reloadMeetings({ silent: true })}
      />
    </div>
  );
}

/** Meeting `id`'s title in the meeting list, if the list has it. */
function listedTitle(list: MeetingList | null, id: string): string | undefined {
  return list?.meetings.find((meeting) => meeting.id === id)?.title;
}

/**
 * One line of the finished transcript. Memoised: a two-hour meeting is
 * thousands of rows, and this screen re-renders on every live-transcript
 * event — including while a *different* meeting records — so without it each
 * event re-rendered every line of this one.
 */
const TranscriptRow = memo(function TranscriptRow({
  line,
  showSpeaker,
}: {
  line: TranscriptLine;
  showSpeaker: boolean;
}) {
  return (
    <li className="transcript__line">
      <time className="transcript__time">{line.time}</time>
      <SpeakerLabel speaker={line.speaker} show={showSpeaker} />
      <span className="transcript__text">
        <span className="sr-only">{line.speaker}: </span>
        {line.text}
      </span>
    </li>
  );
});
