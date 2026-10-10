/**
 * "Record this meeting?" as a card in its own small window (TUR-59, restyled
 * after Granola's in TUR-108 and TUR-147).
 *
 * Rust owns the card (`detection/popup`): it makes this window, sends the
 * card on `prompt-popup://show`, closes it when its time is up, and does
 * what the buttons say. The window shows one of three cards, picked by the
 * same rule as the window's size (`window.rs` `Layout`):
 *
 * * a calendar reminder (TUR-108): a thin accent bar, the meeting's title
 *   over its time range, and one split button: **Join Meet & record** (Zoom,
 *   Teams, or plain "Join" for another service) when the event has a meeting
 *   link, **Record** otherwise. The chevron opens a native menu, since a
 *   72px window cannot hold a dropdown: Join only and Record only (with a
 *   link), Open brief, and Dismiss;
 * * a detection prompt (TUR-147, `StartCard`): "Zoom call", **Record**,
 *   **Not now**, and "⋯" with **Never for Zoom** in the same native menu;
 * * a countdown (TUR-147, `CountdownCard`): "Zoom call ended", a ring
 *   counting down, **Stop now** and **Keep recording**.
 *
 * Nothing records without a click (L15). A recording that starts any other
 * way answers a prompt, and one that stops answers a countdown, so the card
 * closes. Escape is Not now (or Keep recording) once the card has the
 * keyboard. Each card slides in (keyed by its id, so a replacement slides in
 * too) and fades out, except with Reduce Motion.
 */

import { LogicalPosition } from "@tauri-apps/api/dpi";
import { Menu } from "@tauri-apps/api/menu";
import { ChevronDown, Circle, Video } from "lucide-react";
import { type MouseEvent, useEffect, useRef, useState } from "react";
import { onRecordingState } from "@/ipc/client";
import {
  answerPromptPopup,
  onPromptPopup,
  type PopupAnswer,
  type PopupCard,
  type PopupPrompt,
  promptPopupCurrent,
} from "@/ipc/promptPopup";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Icon } from "./icons";
import { buttonVariants } from "./primitives";
import { CountdownCard, FADE_MS, motionClasses, StartCard } from "./promptCards";
import { formatTime } from "./TodayPane";

type Prompt = Extract<PopupCard, { kind: "prompt" }>["prompt"];

/** One item in a card's native menu. */
export type Choice = { answer: PopupAnswer; text: string };

/** The service's name on the main button: "Join Meet", or plain "Join". */
const SERVICE: Record<string, string> = { meet: "Meet", zoom: "Zoom", teams: "Teams" };

/** "Join Meet", "Join Zoom", "Join Teams", or "Join" for any other service. */
export function joinWords(service: string | null): string {
  const name = service === null ? undefined : SERVICE[service];
  return name === undefined ? "Join" : `Join ${name}`;
}

/** The reminder chevron's menu items for `prompt`, in order. */
export function choicesFor(prompt: Prompt): Choice[] {
  const choices: Choice[] = [];
  if (prompt.canJoin) {
    choices.push({ answer: "join", text: "Join only" }, { answer: "record", text: "Record only" });
  }
  if (prompt.signal.kind === "calendar") choices.push({ answer: "openBrief", text: "Open brief" });
  choices.push({ answer: "dismiss", text: "Dismiss" });
  return choices;
}

/** The detection card's "⋯" menu items: Never for the app, when there is one. */
export function startChoicesFor(prompt: Prompt): Choice[] {
  return prompt.app === null ? [] : [{ answer: "neverFor", text: `Never for ${prompt.app}` }];
}

/** Which card `card` is: the same rule as Rust's `window::Layout`. */
export function layoutOf(card: PopupCard): "reminder" | "start" | "countdown" {
  if (card.kind === "countdown") return "countdown";
  return card.prompt.signal.kind === "calendar" ? "reminder" : "start";
}

/** Should the card skip its fade? With Reduce Motion, or where nothing can tell. */
function reducedMotion(): boolean {
  if (typeof window.matchMedia !== "function") return true;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** The reminder card's second line: the time range, the reason, or the question. */
function detailFor(prompt: Prompt): string {
  if (prompt.startsAtMs !== null && prompt.endsAtMs !== null) {
    const range = `${formatTime(prompt.startsAtMs)} to ${formatTime(prompt.endsAtMs)}`;
    return prompt.test ? `${range} (test, nothing records)` : range;
  }
  return prompt.title === null ? "Record this meeting?" : prompt.reason;
}

export function PromptPopup() {
  const [shown, setShown] = useState<PopupPrompt | null>(null);
  const [leaving, setLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shownRef = useRef<PopupPrompt | null>(null);
  shownRef.current = shown;
  /** The last native menu, freed when the next one replaces it. */
  const menuRef = useRef<Menu | null>(null);

  /** Take the card off screen: faded out first, unless motion is reduced. */
  const leave = () => {
    if (reducedMotion()) setShown(null);
    else setLeaving(true);
  };

  const answer = (on: PopupPrompt, pressed: PopupAnswer) => {
    // Join opens the meeting and leaves the question up. A Record stays up
    // until it has started (TUR-169), so a refusal shows on this card.
    const starts = pressed === "record" || pressed === "joinAndRecord";
    if (pressed !== "join" && !starts) leave();
    answerPromptPopup(on.id, pressed)
      .then(() => {
        if (starts && shownRef.current?.id === on.id) leave();
      })
      .catch((thrown: unknown) => {
        setLeaving(false);
        setShown(on);
        setError(toUiError(thrown).message);
      });
  };

  useEffect(() => {
    let live = true;
    promptPopupCurrent()
      .then((current) => {
        if (live && current !== null) setShown((now) => now ?? current);
      })
      .catch(() => {
        // Nothing on screen yet; the event below brings the card.
      });
    const stop = onPromptPopup((next) => {
      setError(null);
      setLeaving(false);
      setShown(next);
    });
    return () => {
      live = false;
      stop();
      freeMenu(menuRef);
    };
  }, []);

  // A recording that starts answers a prompt; one that stops answers a
  // countdown. Either way the question is moot.
  useEffect(
    () =>
      onRecordingState((status) => {
        const now = shownRef.current;
        if (now === null) return;
        const moot =
          now.card.kind === "countdown" ? status.phase === "idle" : status.phase !== "idle";
        if (!moot) return;
        setShown(null);
        answerPromptPopup(now.id, "dismiss").catch(() => {
          // Rust closes it on its own timer anyway.
        });
      }),
    [],
  );

  // Fade out as the card's time runs out; Rust hides the window just after.
  useEffect(() => {
    if (shown === null) return;
    const timer = setTimeout(
      () => {
        if (reducedMotion()) setShown(null);
        else setLeaving(true);
      },
      Math.max(0, shown.closesAtMs - Date.now()),
    );
    return () => clearTimeout(timer);
  }, [shown]);

  // Gone once faded.
  useEffect(() => {
    if (!leaving) return;
    const timer = setTimeout(() => {
      setShown(null);
      setLeaving(false);
    }, FADE_MS);
    return () => clearTimeout(timer);
  }, [leaving]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const now = shownRef.current;
      if (event.key !== "Escape" || now === null) return;
      answer(now, now.card.kind === "countdown" ? "keepRecording" : "dismiss");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (shown === null) return null;

  const press = (pressed: PopupAnswer) => answer(shown, pressed);

  const openMenu = (event: MouseEvent<HTMLButtonElement>, choices: Choice[]) => {
    const box = event.currentTarget.getBoundingClientRect();
    const items = choices.map((choice) => ({
      id: choice.answer,
      text: choice.text,
      action: () => press(choice.answer),
    }));
    Menu.new({ items })
      .then((menu) => {
        freeMenu(menuRef);
        menuRef.current = menu;
        // Just under the button, from the window's top-left corner.
        return menu.popup(new LogicalPosition(box.left, box.bottom));
      })
      .catch(() => setError("Could not open the menu. The card closes on its own."));
  };

  const { card } = shown;
  if (card.kind === "countdown") {
    return (
      <CountdownCard
        key={shown.id}
        line={card.line}
        seconds={card.seconds}
        closesAtMs={shown.closesAtMs}
        leaving={leaving}
        error={error}
        onAnswer={press}
      />
    );
  }
  const { prompt } = card;
  if (layoutOf(card) === "start") {
    return (
      <StartCard
        key={shown.id}
        headline={prompt.headline}
        app={prompt.app}
        leaving={leaving}
        error={error}
        onAnswer={press}
        onMenu={(event) => openMenu(event, startChoicesFor(prompt))}
      />
    );
  }

  const join = prompt.canJoin;
  const mainWords = join ? joinWords(prompt.joinService) : "Record";
  const heading = prompt.title ?? prompt.reason;

  return (
    <section
      key={shown.id}
      aria-labelledby="prompt-popup-title"
      aria-live="polite"
      data-leaving={leaving ? "" : undefined}
      className={cn(
        "flex h-screen items-center gap-5 overflow-hidden rounded-card border-[0.5px] border-rim bg-popup py-4 pr-4 pl-4 text-fg-primary",
        motionClasses(leaving),
      )}
    >
      <span
        aria-hidden="true"
        data-testid="prompt-popup-bar"
        className="w-2 shrink-0 self-stretch rounded-capsule bg-accent"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <h2 id="prompt-popup-title" className="truncate text-headline font-semibold">
          {heading}
        </h2>
        <p
          className={cn("truncate text-footnote", error ? "text-danger" : "text-fg-secondary")}
          role={error ? "alert" : undefined}
        >
          {error ?? detailFor(prompt)}
        </p>
      </div>
      <div className="flex shrink-0 items-stretch">
        <button
          type="button"
          aria-label={join ? `${mainWords} and record` : mainWords}
          className={cn(
            buttonVariants({ tone: "primary", size: "small" }),
            "h-(--control-h-touch) rounded-r-none",
          )}
          onClick={() => press(join ? "joinAndRecord" : "record")}
        >
          <Icon icon={join ? Video : Circle} />
          {join ? (
            <span className="flex flex-col items-start leading-tight">
              <span>{mainWords}</span>
              <span className="text-caption1 font-normal">&amp; record</span>
            </span>
          ) : (
            mainWords
          )}
        </button>
        <button
          type="button"
          aria-label="More choices"
          aria-haspopup="menu"
          title="More choices"
          className={cn(
            buttonVariants({ tone: "primary", size: "small" }),
            "h-(--control-h-touch) rounded-l-none border-l-on-accent/30 px-2",
          )}
          onClick={(event) => openMenu(event, choicesFor(prompt))}
        >
          <Icon icon={ChevronDown} />
        </button>
      </div>
    </section>
  );
}

/** Free the last native menu, if any; it is never shown again. */
function freeMenu(menuRef: { current: Menu | null }) {
  const menu = menuRef.current;
  menuRef.current = null;
  menu?.close().catch(() => {
    // Already gone with the window; nothing to free.
  });
}
