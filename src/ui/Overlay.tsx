/**
 * The recording overlay (TUR-146): a small card in its own always-on-top
 * window, so a user in Meet, Zoom or a doc can see the recording is live and
 * pause or stop it without switching back to meet-ai.
 *
 * It shows only three things: a timer (`mm:ss`, `h:mm:ss` past an hour,
 * paused stretches left out), the last line or two of the live transcript
 * (no speaker labels, no history), and Pause or Resume and Stop. A click on
 * the text brings the main window forward. Everything else about it is
 * Rust's (`src-tauri/src/overlay`): it shows the window when a recording
 * starts, takes it down when it ends (on macOS hidden and kept, so this page
 * lives on between recordings), and remembers where it was dragged.
 *
 * The state is the same two mirrors the main window keeps
 * ({@link watchRecordingState}, {@link watchLiveTranscript}), so the two
 * windows can never disagree, and Stop here is the same Stop as there.
 */

import { Pause, Play, Square } from "lucide-react";
import { useEffect } from "react";
import { isPaused, overlayShowMain } from "@/ipc/client";
import type { LiveLine } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { formatTimer } from "@/lib/elapsed";
import { useRecordingStore, watchRecordingState } from "@/state/recording";
import { type LiveTranscript, useTranscriptStore, watchLiveTranscript } from "@/state/transcript";
import { IconButton } from "./primitives";
import { useRecordedMs } from "./useRecordedMs";

/** How many of the latest lines the card shows. */
export const OVERLAY_LINES = 2;

/**
 * The latest `count` lines, oldest first: the settled lines and each
 * speaker's in-progress guess, in `seq` order.
 */
export function latestLines(live: LiveTranscript, count = OVERLAY_LINES): LiveLine[] {
  const guesses = [live.volatile.you, live.volatile.others].filter(
    (line): line is LiveLine => line !== null,
  );
  return [...live.finals.slice(-count), ...guesses].sort((a, b) => a.seq - b.seq).slice(-count);
}

/** The words in the text row when there are no lines to show. */
export function emptyText(paused: boolean): string {
  return paused
    ? "Paused. Nothing is recorded until you resume."
    : "Recording. The latest words show here.";
}

export function Overlay() {
  const status = useRecordingStore((state) => state.status);
  const busy = useRecordingStore((state) => state.busy);
  const error = useRecordingStore((state) => state.error);
  const togglePause = useRecordingStore((state) => state.togglePause);
  const stop = useRecordingStore((state) => state.stop);
  const live = useTranscriptStore((state) => state.live);
  const elapsed = useRecordedMs(status);

  useEffect(() => {
    const stopRecording = watchRecordingState();
    const stopTranscript = watchLiveTranscript();
    return () => {
      stopTranscript();
      stopRecording();
    };
  }, []);

  const recording = status.phase === "recording";
  const paused = isPaused(status);
  const lines = paused ? [] : latestLines(live);
  const timer = formatTimer(elapsed);

  return (
    <section
      aria-label="meet-ai recording"
      data-tauri-drag-region
      className="glass flex h-screen flex-col justify-center gap-2 overflow-hidden px-5 py-4 text-fg-primary"
    >
      <div className="flex items-center gap-4" data-tauri-drag-region>
        <span className="flex text-recording" aria-hidden="true" data-tauri-drag-region>
          <span
            data-testid="overlay-dot"
            className={paused ? "record__dot" : "record__dot record__dot--live"}
          />
        </span>
        <span
          role="timer"
          className="text-headline font-semibold tabular-nums"
          aria-label={`${paused ? "Paused at" : "Recording for"} ${timer}`}
          data-tauri-drag-region
        >
          {timer}
        </span>
        {paused ? (
          <span className="text-footnote text-fg-secondary" data-tauri-drag-region>
            Paused
          </span>
        ) : null}
        <span className="flex-1 self-stretch" data-tauri-drag-region />
        <IconButton
          icon={paused ? Play : Pause}
          label={paused ? "Resume recording" : "Pause recording"}
          tone="neutral"
          className="size-(--control-h-regular) rounded-capsule"
          disabled={busy || !recording}
          onClick={() => void togglePause()}
        />
        <IconButton
          icon={Square}
          label="Stop recording"
          tone="neutral"
          className="size-(--control-h-regular) rounded-capsule bg-danger-fill text-on-accent not-disabled:hover:bg-danger-fill not-disabled:hover:text-on-accent"
          disabled={busy || !recording}
          onClick={() => void stop()}
        />
      </div>

      {error ? (
        <p role="alert" className="truncate text-footnote text-danger">
          {error.message}
        </p>
      ) : (
        <button
          type="button"
          title="Open meet-ai"
          className="flex min-w-0 cursor-default flex-col items-start text-left text-footnote leading-tight"
          onClick={() => {
            overlayShowMain().catch(() => {
              // The main window could not come forward; nothing to say here.
            });
          }}
        >
          <span className="sr-only">Open meet-ai. </span>
          {lines.length === 0 ? (
            <span className="truncate text-fg-secondary">{emptyText(paused)}</span>
          ) : (
            lines.map((line, index) => (
              <span
                key={line.seq}
                data-testid="overlay-line"
                className={cn(
                  "w-full truncate",
                  index < lines.length - 1 ? "text-fg-secondary" : "text-fg-primary",
                )}
              >
                {line.text}
              </span>
            ))
          )}
        </button>
      )}
    </section>
  );
}
