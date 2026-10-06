/**
 * The live transcript pane, shown in place of the finished transcript while
 * the open meeting is the one recording.
 *
 * Three rules shape it:
 *
 * * **Volatile versus final is the signal** (MASTER.md §5.4). A speaker's
 *   in-progress guess renders muted and italic, and is replaced in place until
 *   it settles; only settled lines reach `transcript.md` (SPEC §2.5).
 * * **No layout jump, and no yanking** (SPEC §5 Phase 2). The pane is a fixed
 *   height, so lines arriving never move the notes below it. It follows the
 *   newest line only while the reader is already at the bottom; scrolled up to
 *   read, they stay put and get a button back down instead.
 * * **Transcription failing is not recording failing** (TUR-96). The audio is
 *   what cannot be recovered later; a transcript can be re-run from the WAVs.
 *   So a failure is a notice that says exactly that, not an error screen.
 */

import { memo, useLayoutEffect, useRef, useState } from "react";
import type { LiveLine, TranscriptStatus } from "@/ipc/types";
import { formatElapsed } from "@/lib/format";
import type { LiveTranscript as LiveState } from "@/state/transcript";
import { Button } from "./primitives";
import { SpeakerLabel } from "./SpeakerLabel";

/**
 * How close to the bottom still counts as "at the bottom", in pixels. Enough
 * to absorb a sub-pixel scroll position and a trackpad that settles a few
 * pixels short, small enough that a deliberate scroll up of one line counts.
 */
export const FOLLOW_SLACK_PX = 24;

/** Should the pane keep following new lines, given where the reader is now? */
export function isAtBottom(
  position: { scrollTop: number; scrollHeight: number; clientHeight: number },
  slack: number = FOLLOW_SLACK_PX,
): boolean {
  return position.scrollHeight - position.scrollTop - position.clientHeight <= slack;
}

export function LiveTranscript({ live }: { live: LiveState }) {
  const scroller = useRef<HTMLDivElement>(null);
  // A ref, not state: it is read after every render and changing it must not
  // cause one. It is decided at the last *user* scroll, so content growing
  // under a reader who was at the bottom does not un-pin them.
  const following = useRef(true);
  const seenFinals = useRef(live.finals.length);
  const [unseen, setUnseen] = useState(false);

  const guesses = [live.volatile.you, live.volatile.others]
    .filter((line): line is LiveLine => line !== null)
    .sort((a, b) => a.seq - b.seq);

  // Layout, not a plain effect: the scroll has to land before paint, or the
  // reader sees the new line appear below the fold and then jump into view.
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    if (following.current) {
      element.scrollTop = element.scrollHeight;
      seenFinals.current = live.finals.length;
    } else if (live.finals.length > seenFinals.current) {
      setUnseen(true);
    }
  });

  function handleScroll() {
    const element = scroller.current;
    if (!element) return;
    following.current = isAtBottom(element);
    if (following.current) {
      seenFinals.current = live.finals.length;
      setUnseen(false);
    }
  }

  function jumpToLatest() {
    const element = scroller.current;
    if (!element) return;
    following.current = true;
    seenFinals.current = live.finals.length;
    element.scrollTop = element.scrollHeight;
    setUnseen(false);
  }

  const empty = live.finals.length === 0 && guesses.length === 0;

  return (
    <section className="section" aria-labelledby="live-heading">
      <div className="section__header">
        <h2 className="section__title" id="live-heading">
          Live transcript
        </h2>
        <p className="section__hint">{hintFor(live.status)}</p>
      </div>

      {live.status.state === "failed" ? <FailedNotice detail={live.status.detail} /> : null}

      <div className="live">
        <div className="live__scroller relative" ref={scroller} onScroll={handleScroll}>
          {empty ? <p className="live__empty">{emptyCopyFor(live.status)}</p> : null}

          {/* Only settled lines are announced. A guess is rewritten several
              times a second while someone talks, and reading each rewrite
              aloud would drown out the meeting itself. */}
          <div role="log" aria-live="polite" aria-relevant="additions" aria-label="Live transcript">
            <ol className="transcript">
              {live.finals.map((line, i) => (
                <LiveRow
                  key={line.seq}
                  line={line}
                  volatile={false}
                  showSpeaker={i === 0 || live.finals[i - 1]?.speaker !== line.speaker}
                />
              ))}
            </ol>
          </div>

          {guesses.length > 0 ? (
            <ol className="transcript live__guesses" aria-live="off">
              {guesses.map((line) => (
                <LiveRow key={line.speaker} line={line} volatile showSpeaker />
              ))}
            </ol>
          ) : null}
        </div>

        {/* Floats over the pane rather than taking a row of its own, so it
            appearing does not shift the lines the reader is looking at. */}
        {unseen ? (
          <Button size="small" className="live__jump" onClick={jumpToLatest}>
            New lines below
          </Button>
        ) : null}
      </div>
    </section>
  );
}

/**
 * One live line. Memoised because the pane re-renders on every event — a
 * guess is rewritten several times a second while someone talks — and each
 * settled line keeps its object identity across those, so only the row that
 * changed renders again.
 */
const LiveRow = memo(function LiveRow({
  line,
  volatile,
  showSpeaker,
}: {
  line: LiveLine;
  volatile: boolean;
  showSpeaker: boolean;
}) {
  const label = line.speaker === "you" ? "You" : "Others";
  return (
    <li className="transcript__line" data-volatile={volatile || undefined}>
      <time className="transcript__time">{formatElapsed(line.start_sec * 1000)}</time>
      {/* Same chip as the review screen, and the full label for VoiceOver. */}
      <SpeakerLabel speaker={label} show={showSpeaker} />
      <span className="transcript__text">
        <span className="sr-only">
          {label}
          {volatile ? " (still speaking)" : ""}:{" "}
        </span>
        {line.text}
      </span>
    </li>
  );
});

function FailedNotice({ detail }: { detail: string | null }) {
  return (
    <div className="live__notice" role="alert">
      <p className="live__notice-text">
        Transcription stopped, but recording is still going. The audio is being saved as normal. You
        can make the transcript from it after the meeting.
      </p>
      {detail ? <p className="live__notice-detail">{detail}</p> : null}
    </div>
  );
}

function engineName(engine: string | null): string | null {
  switch (engine) {
    case null:
      return null;
    case "apple-speech":
      return "Apple Speech";
    case "whisper":
      return "Whisper";
    default:
      return engine;
  }
}

function hintFor(status: TranscriptStatus): string {
  const engine = engineName(status.engine);
  switch (status.state) {
    case "idle":
      return "Starting transcription…";
    case "running":
      return engine
        ? `Transcribing on this Mac with ${engine}. Finished lines are saved to transcript.md`
        : "Transcribing on this Mac. Finished lines are saved to transcript.md";
    case "stopped":
      return "Transcription finished";
    case "failed":
      return "Transcription stopped";
  }
}

function emptyCopyFor(status: TranscriptStatus): string {
  switch (status.state) {
    case "idle":
      return "Getting the speech engine ready. Lines will appear here once it is listening.";
    case "running":
      return "Listening. Lines appear here a couple of seconds after someone speaks.";
    case "stopped":
      return "Nothing was said while transcription was running.";
    case "failed":
      return "Nothing was transcribed before it stopped.";
  }
}
