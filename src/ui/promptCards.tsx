/**
 * The narrow prompt cards (TUR-147), laid out after Granola's "Take notes"
 * card and styled with our TUR-102 pieces: our app icon, one line, a big
 * primary button and a quiet one under it.
 *
 * * {@link StartCard}: "Zoom call", **Record**, **Not now**, and a "⋯"
 *   button whose native menu holds **Never for Zoom**.
 * * {@link CountdownCard}: "Zoom call ended", a ring counting down to the
 *   stop, **Stop now** and **Keep recording**.
 *
 * Both are 220 x 120 logical px (`detection/popup/window.rs`), inside a
 * window that is 8px bigger on every side off macOS so the card's own shadow
 * has room; macOS draws the window shadow itself, so the card has none there.
 * `PromptPopup` owns the state, the answers and the menu.
 */

import { Circle, Ellipsis, Square } from "lucide-react";
import { type MouseEvent, type ReactNode, useEffect, useState } from "react";
import type { PopupAnswer } from "@/ipc/promptPopup";
import { cn } from "@/lib/cn";
import appIcon from "../../design-system/meet-ai/brand/meet-ai-appicon-small-fullcolor.svg";
import { Button, IconButton } from "./primitives";

/** How long the card takes to fade out, in ms (`--dur-fast`). Rust hides the window a little later. */
export const FADE_MS = 140;

/**
 * Slide in from the right as the card appears, fade out as it leaves; both
 * off with Reduce Motion, where it just appears and goes.
 */
export function motionClasses(leaving: boolean): string {
  return cn(
    "transition-[opacity,translate] duration-(--dur-normal) ease-out motion-reduce:transition-none",
    "motion-safe:starting:translate-x-4 motion-safe:starting:opacity-0",
    leaving && "opacity-0 duration-(--dur-fast)",
  );
}

/** The narrow card's frame: the window's inset, the rounded surface, the shadow. */
function NarrowCard({
  leaving,
  label,
  children,
}: {
  leaving: boolean;
  /** The id of the card's line, which names the card for VoiceOver. */
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="h-screen p-4 [[data-os=macos]_&]:p-0">
      <section
        aria-labelledby={label}
        aria-live="polite"
        data-leaving={leaving ? "" : undefined}
        className={cn(
          "flex h-full flex-col gap-4 overflow-hidden rounded-card border-[0.5px] border-rim bg-popup p-5 text-fg-primary",
          "shadow-floating [[data-os=macos]_&]:shadow-none",
          motionClasses(leaving),
        )}
      >
        {children}
      </section>
    </div>
  );
}

/** Our icon and the card's one line, with whatever sits at the end of the row. */
function Header({
  id,
  line,
  error,
  end,
}: {
  id: string;
  line: string;
  error: string | null;
  end?: ReactNode;
}) {
  return (
    <div className="flex h-(--control-h-regular) shrink-0 items-center gap-4">
      {/* Decorative: the line beside it says what the card is about. */}
      <img src={appIcon} alt="" width={20} height={20} draggable={false} className="shrink-0" />
      <h2 id={id} className="min-w-0 flex-1 truncate text-callout font-semibold" title={line}>
        {error === null ? (
          line
        ) : (
          <span role="alert" className="text-danger" title={error}>
            {error}
          </span>
        )}
      </h2>
      {end}
    </div>
  );
}

/** "Zoom call": Record, Not now, and the "⋯" menu with Never for Zoom. */
export function StartCard({
  headline,
  app,
  leaving,
  error,
  onAnswer,
  onMenu,
}: {
  headline: string;
  /** The app to offer "Never for" about; no "⋯" without one. */
  app: string | null;
  leaving: boolean;
  error: string | null;
  onAnswer: (answer: PopupAnswer) => void;
  onMenu: (event: MouseEvent<HTMLButtonElement>) => void;
}) {
  return (
    <NarrowCard leaving={leaving} label="prompt-popup-title">
      <Header
        id="prompt-popup-title"
        line={headline}
        error={error}
        end={
          app === null ? null : (
            <IconButton
              icon={Ellipsis}
              label="More choices"
              aria-haspopup="menu"
              className="-mr-2 size-(--control-h-regular)"
              onClick={onMenu}
            />
          )
        }
      />
      <Button
        tone="primary"
        block
        icon={Circle}
        aria-label={`Record ${headline}`}
        onClick={() => onAnswer("record")}
      >
        Record
      </Button>
      <Button
        tone="quiet"
        size="small"
        block
        aria-label="Not now, do not record"
        onClick={() => onAnswer("dismiss")}
      >
        Not now
      </Button>
    </NarrowCard>
  );
}

/** Whole seconds left until `closesAtMs`, never above `seconds` or below 0. */
export function secondsLeft(closesAtMs: number, seconds: number, nowMs: number): number {
  const left = Math.ceil((closesAtMs - nowMs) / 1000);
  return Math.min(seconds, Math.max(0, left));
}

/** How often the countdown looks at the clock, in ms. */
const TICK_MS = 250;
/** The ring's radius in its 24 x 24 box, and the length of its stroke. */
const RING_R = 10;
const RING_LENGTH = 2 * Math.PI * RING_R;

/** The ring around the seconds left, emptying as they run out. */
function CountdownRing({ left, seconds }: { left: number; seconds: number }) {
  const full = seconds > 0 ? left / seconds : 0;
  return (
    <span
      role="timer"
      aria-label={`Stopping in ${left} ${left === 1 ? "second" : "seconds"}`}
      className="relative grid size-(--control-h-regular) shrink-0 place-items-center"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true" className="absolute inset-0 -rotate-90">
        <circle cx="12" cy="12" r={RING_R} strokeWidth="2" className="fill-none stroke-separator" />
        <circle
          cx="12"
          cy="12"
          r={RING_R}
          strokeWidth="2"
          strokeLinecap="round"
          strokeDasharray={RING_LENGTH}
          strokeDashoffset={RING_LENGTH * (1 - full)}
          data-testid="countdown-ring"
          className="fill-none stroke-accent transition-[stroke-dashoffset] duration-1000 ease-linear motion-reduce:transition-none"
        />
      </svg>
      <span aria-hidden="true" className="text-caption2 font-semibold tabular-nums">
        {left}
      </span>
    </span>
  );
}

/** "Zoom call ended": a ring counting down, Stop now, Keep recording. */
export function CountdownCard({
  line,
  seconds,
  closesAtMs,
  leaving,
  error,
  onAnswer,
}: {
  line: string;
  seconds: number;
  closesAtMs: number;
  leaving: boolean;
  error: string | null;
  onAnswer: (answer: PopupAnswer) => void;
}) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(tick);
  }, []);
  const left = secondsLeft(closesAtMs, seconds, now);
  return (
    <NarrowCard leaving={leaving} label="prompt-popup-title">
      <Header
        id="prompt-popup-title"
        line={line}
        error={error}
        end={<CountdownRing left={left} seconds={seconds} />}
      />
      <Button
        tone="primary"
        block
        icon={Square}
        aria-label="Stop now and save the recording"
        onClick={() => onAnswer("stopNow")}
      >
        Stop now
      </Button>
      <Button tone="quiet" size="small" block onClick={() => onAnswer("keepRecording")}>
        Keep recording
      </Button>
    </NarrowCard>
  );
}
