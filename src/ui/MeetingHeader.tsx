/**
 * The top of the meeting view (TUR-81): the title, one quiet meta line under
 * it, and the folder actions on the right of the title row. Nothing else —
 * the notes switch lives with the meeting's notes, not here.
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
import { Button, rowDetailVariants } from "./primitives";
import { useElapsed } from "./RecordControl";
import { ErrorState } from "./states";

export const SHOW_IN_FINDER_LABEL = "Show in Finder";
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
  revealError,
}: {
  summary: MeetingSummary;
  /** The meeting folder on disk: the tooltip, and what Copy folder path copies. */
  path: string;
  /** This meeting's recording status while it is the one recording, else `null`. */
  live: RecordingStatus | null;
  onReveal: () => void;
  /** Finder could not open the folder: shown under the header, not instead of it. */
  revealError: UiError | null;
}) {
  const interrupted = summary.recordingState === "interrupted";
  // The clipboard said no to Copy folder path: the path, selectable, instead.
  const [copyRefused, setCopyRefused] = useState(false);

  return (
    <header className="flex flex-col gap-4">
      <div className="flex items-start justify-between gap-5">
        <div className="flex min-w-0 flex-col gap-2">
          <h1 className="page__title wrap-anywhere">{summary.title}</h1>
          <MetaLine summary={summary} live={live} interrupted={interrupted} />
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <IconButton label={SHOW_IN_FINDER_LABEL} tooltip={path} onClick={onReveal}>
            <FolderIcon />
          </IconButton>
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
  parts.push(formatRelativeDate(summary.date));
  if (summary.time) parts.push(summary.time);
  if (live && live.phase !== "idle") {
    if (live.startedAtMs !== null) {
      parts.push(<LiveDuration key="elapsed" startedAtMs={live.startedAtMs} />);
    }
  } else {
    // A5: the WAV header is the true length; the last line stands in once
    // retention has deleted the audio.
    const length = summary.audioMs ?? timestampToMs(summary.lastTimestamp);
    if (length !== null) parts.push(formatDuration(length));
  }
  if (summary.hasAnalysis && !live) parts.push("Wrapped up");
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

/** The live length, ticking on its own so the rest of the page does not re-render each second. */
function LiveDuration({ startedAtMs }: { startedAtMs: number }) {
  return <>{formatDuration(useElapsed(startedAtMs))}</>;
}

function IconButton({
  label,
  tooltip,
  onClick,
  children,
}: {
  label: string;
  tooltip: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <Button
      tone="quiet"
      size="small"
      aria-label={label}
      title={tooltip}
      onClick={onClick}
      className="w-(--control-h-regular) px-0"
    >
      {children}
    </Button>
  );
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
        label={copied ? "Copied" : COPY_PATH_LABEL}
        tooltip={copied ? "Copied" : COPY_PATH_LABEL}
        onClick={() => void copy()}
      >
        {copied ? <CheckIcon /> : <CopyIcon />}
      </IconButton>
      {/* Heard, not seen: the check mark is the visible half. */}
      <span className="sr-only" role="status" aria-live="polite">
        {copied ? "Folder path copied to the clipboard" : ""}
      </span>
    </>
  );
}

// --- icons ----------------------------------------------------------------
// 16px, one stroke weight, `currentColor`, so they follow the button's text
// colour in light, dark and Increase Contrast. Drawn for meet-ai.

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

function FolderIcon() {
  return (
    <Icon>
      <path d="M1.75 4.25a1 1 0 0 1 1-1h3.1l1.5 1.5h5.9a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z" />
    </Icon>
  );
}

function CopyIcon() {
  return (
    <Icon>
      <rect x="5.25" y="5.25" width="8.5" height="8.5" rx="1.5" />
      <path d="M10.75 5.25v-1.5a1.5 1.5 0 0 0-1.5-1.5h-5.5a1.5 1.5 0 0 0-1.5 1.5v5.5a1.5 1.5 0 0 0 1.5 1.5h1.5" />
    </Icon>
  );
}

function CheckIcon() {
  return (
    <Icon>
      <path d="M3 8.5l3.25 3.25L13 5" />
    </Icon>
  );
}
