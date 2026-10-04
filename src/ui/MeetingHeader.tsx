/**
 * The top of the meeting view (TUR-81): the title, one quiet meta line under
 * it, and the folder actions on the right of the title row. Nothing else —
 * the notes switch lives with the meeting's notes, not here.
 *
 * TUR-103: the title renames in place ({@link MeetingTitle}).
 *
 * TUR-102: the title is big and bold with room around it; each meta fact
 * (date, time, attendees, audio length) is quiet, led by a small icon that
 * is decoration beside the word. The folder actions are the shared
 * {@link IconButton}, each named for VoiceOver.
 *
 * The meta line reads "Today · 22:36 · 50 min". While this meeting records,
 * its live state leads that line ("● Recording · 12 min"), with the length
 * from the window's own timer; the pulsing dot is never the only signal, the
 * word is there too (MASTER.md §5.6).
 *
 * The folder path is not printed: it is the Show in Finder button's tooltip,
 * and Copy folder path puts it on the clipboard. Both are small quiet icon
 * buttons with an accessible name (MASTER.md §5.2).
 */

import { Calendar, Check, Clock, Copy, FolderOpen, Timer, Users } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { MeetingSummary, RecordingStatus, UiError } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { cn } from "@/lib/cn";
import { COPIED_RESET_MS } from "@/lib/constants";
import {
  describeInterruption,
  formatDuration,
  formatRelativeDate,
  INTERRUPTED_LABEL,
  timestampToMs,
} from "@/lib/format";
import { osText } from "@/lib/osText";
import { Icon, type LucideIcon } from "./icons";
import { MeetingTitle } from "./MeetingTitle";
import { IconButton, rowDetailVariants } from "./primitives";
import { useElapsed } from "./RecordControl";
import { ErrorState } from "./states";

export const SHOW_IN_FILE_MANAGER_LABEL = `Show in ${osText("fileManager")}`;
export const COPY_PATH_LABEL = "Copy folder path";

/** What the meta line says while this meeting's recording is live. */
const LIVE_WORDS: Record<Exclude<RecordingStatus["phase"], "idle">, string> = {
  starting: "Starting…",
  recording: "Recording",
  stopping: "Stopping…",
};

export function MeetingHeader({
  summary,
  path,
  live,
  onReveal,
  onRename,
  revealError,
}: {
  summary: MeetingSummary;
  /** The meeting folder on disk: the tooltip, and what Copy folder path copies. */
  path: string;
  /** This meeting's recording status while it is the one recording, else `null`. */
  live: RecordingStatus | null;
  onReveal: () => void;
  /** Saves a new title for the meeting; rejects with the reason it could not. */
  onRename: (title: string) => Promise<void>;
  /** Finder could not open the folder: shown under the header, not instead of it. */
  revealError: UiError | null;
}) {
  const interrupted = summary.recordingState === "interrupted";
  // The clipboard said no to Copy folder path: the path, selectable, instead.
  const [copyRefused, setCopyRefused] = useState(false);

  return (
    <header className="flex flex-col gap-5">
      <div className="flex items-start justify-between gap-5">
        <div className="flex min-w-0 flex-col gap-3">
          <MeetingTitle title={summary.title} onRename={onRename} />
          <MetaLine summary={summary} live={live} interrupted={interrupted} />
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <IconButton
            icon={FolderOpen}
            label={SHOW_IN_FILE_MANAGER_LABEL}
            title={path}
            onClick={onReveal}
          />
          <CopyPathButton path={path} onRefused={setCopyRefused} />
        </div>
      </div>
      {/* TUR-97: an interrupted meeting opens like any other, but says in one
          line that it did not end on purpose, and how much of it was kept. */}
      {interrupted ? <p className="page__notice">{describeInterruption(summary.audioMs)}</p> : null}
      {copyRefused ? <p className={cn(rowDetailVariants(), "m-0 select-all")}>{path}</p> : null}
      {revealError ? <ErrorState error={revealError} /> : null}
    </header>
  );
}

/** One quiet fact on the meta line: a small icon and its word. */
function Fact({ icon, children }: { icon?: LucideIcon; children: ReactNode }) {
  return (
    <span className="inline-flex items-center gap-2">
      {icon ? <Icon icon={icon} className="text-fg-tertiary" /> : null}
      {children}
    </span>
  );
}

/** "Today · 22:36 · 50 min", plus the meeting's state words. */
function MetaLine({
  summary,
  live,
  interrupted,
}: {
  summary: MeetingSummary;
  live: RecordingStatus | null;
  interrupted: boolean;
}) {
  const parts: ReactNode[] = [];
  if (live && live.phase !== "idle") {
    parts.push(
      <span key="live" className="inline-flex items-center gap-2 font-medium text-recording">
        <span
          aria-hidden="true"
          className={cn("record__dot", live.phase === "recording" && "record__dot--live")}
        />
        {LIVE_WORDS[live.phase]}
      </span>,
    );
  }
  parts.push(<Fact icon={Calendar}>{formatRelativeDate(summary.date)}</Fact>);
  if (summary.time) parts.push(<Fact icon={Clock}>{summary.time}</Fact>);
  if (summary.attendees.length > 0) {
    parts.push(<Fact icon={Users}>{attendeesWord(summary.attendees)}</Fact>);
  }
  if (live && live.phase !== "idle") {
    if (live.startedAtMs !== null) {
      parts.push(
        <Fact key="elapsed" icon={Timer}>
          <LiveDuration startedAtMs={live.startedAtMs} />
        </Fact>,
      );
    }
  } else {
    // A5: the WAV header is the true length; the last line stands in once
    // retention has deleted the audio.
    const length = summary.audioMs ?? timestampToMs(summary.lastTimestamp);
    if (length !== null) parts.push(<Fact icon={Timer}>{formatDuration(length)}</Fact>);
  }
  if (summary.hasAnalysis && !live) parts.push(<Fact icon={Check}>Wrapped up</Fact>);
  if (interrupted) parts.push(INTERRUPTED_LABEL);

  return (
    <p
      className="m-0 flex flex-wrap items-center gap-x-2 text-footnote tabular-nums text-fg-secondary contrast-more:text-fg-primary"
      data-testid="meeting-meta"
    >
      {parts.map((part, index) => (
        // Positions are stable within one render's list; the parts never reorder.
        // biome-ignore lint/suspicious/noArrayIndexKey: see above
        <span key={index} className="inline-flex items-center gap-2">
          {index > 0 ? <span aria-hidden="true">·</span> : null}
          {part}
        </span>
      ))}
    </p>
  );
}

/** "3 people", or the one name when a meeting names exactly one. */
function attendeesWord(attendees: string[]): string {
  if (attendees.length === 1) return attendees[0] ?? "1 person";
  return `${attendees.length} people`;
}

/** The live length, ticking on its own so the rest of the page does not re-render each second. */
function LiveDuration({ startedAtMs }: { startedAtMs: number }) {
  return <>{formatDuration(useElapsed(startedAtMs))}</>;
}

/**
 * Copy folder path, with a brief "Copied" state. A clipboard that says no is
 * not an error screen: the path is shown, selectable, to copy by hand.
 */
function CopyPathButton({
  path,
  onRefused,
}: {
  path: string;
  onRefused: (refused: boolean) => void;
}) {
  const [copied, setCopied] = useState(false);
  const resetTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  async function copy() {
    window.clearTimeout(resetTimer.current);
    setCopied(false);
    try {
      await copyText(path);
    } catch {
      onRefused(true);
      return;
    }
    onRefused(false);
    setCopied(true);
    resetTimer.current = window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
  }

  return (
    <>
      <IconButton
        icon={copied ? Check : Copy}
        label={copied ? "Copied" : COPY_PATH_LABEL}
        title={copied ? "Copied" : COPY_PATH_LABEL}
        onClick={() => void copy()}
      />
      {/* Heard, not seen: the check mark is the visible half. */}
      <span className="sr-only" role="status" aria-live="polite">
        {copied ? "Folder path copied to the clipboard" : ""}
      </span>
    </>
  );
}
