/**
 * "Record this meeting?" as a compact card in its own small window (TUR-59,
 * restyled after Granola's reminder in TUR-108): a thin accent bar, the
 * meeting's title over its time range, and one split button.
 *
 * Rust owns the prompt (`detection/popup`): it makes this window, sends the
 * prompt on `prompt-popup://show`, hides it after a while, and does what the
 * buttons say. A calendar reminder shows here on every OS; a detection
 * prompt ("Zoom is open.") on Windows and Linux, with the reason as its title.
 *
 * The main button is **Join Meet & record** (Zoom, Teams, or plain "Join"
 * for another service) when the event has a meeting link, and **Record**
 * otherwise. The chevron beside it opens a native menu, since a 72px window
 * cannot hold a dropdown: Join only and Record only (with a link), Open brief
 * (a reminder), and Dismiss. Nothing records without a click (L15). A
 * recording that starts any other way answers the question, so the card
 * closes.
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
  type PopupPrompt,
  promptPopupCurrent,
} from "@/ipc/promptPopup";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Icon } from "./icons";
import { buttonVariants } from "./primitives";
import { formatTime } from "./TodayPane";

type Prompt = PopupPrompt["prompt"];

/** One item in the chevron's menu. */
export type Choice = { answer: PopupAnswer; text: string };

/** The service's name on the main button: "Join Meet", or plain "Join". */
const SERVICE: Record<string, string> = { meet: "Meet", zoom: "Zoom", teams: "Teams" };

/** "Join Meet", "Join Zoom", "Join Teams", or "Join" for any other service. */
export function joinWords(service: string | null): string {
  const name = service === null ? undefined : SERVICE[service];
  return name === undefined ? "Join" : `Join ${name}`;
}

/** The chevron menu's items for `prompt`, in order. */
export function choicesFor(prompt: Prompt): Choice[] {
  const choices: Choice[] = [];
  if (prompt.canJoin) {
    choices.push({ answer: "join", text: "Join only" }, { answer: "record", text: "Record only" });
  }
  if (prompt.signal.kind === "calendar") choices.push({ answer: "openBrief", text: "Open brief" });
  choices.push({ answer: "dismiss", text: "Dismiss" });
  return choices;
}

/** The card's second line: the time range, the reason, or the question. */
function detailFor(prompt: Prompt): string {
  if (prompt.startsAtMs !== null && prompt.endsAtMs !== null) {
    const range = `${formatTime(prompt.startsAtMs)} to ${formatTime(prompt.endsAtMs)}`;
    return prompt.test ? `${range} (test, nothing records)` : range;
  }
  return prompt.title === null ? "Record this meeting?" : prompt.reason;
}

export function PromptPopup() {
  const [shown, setShown] = useState<PopupPrompt | null>(null);
  const [error, setError] = useState<string | null>(null);
  const shownRef = useRef<PopupPrompt | null>(null);
  shownRef.current = shown;
  /** The last native menu, freed when the next one replaces it. */
  const menuRef = useRef<Menu | null>(null);

  useEffect(() => {
    let live = true;
    promptPopupCurrent()
      .then((current) => {
        if (live && current !== null) setShown((now) => now ?? current);
      })
      .catch(() => {
        // Nothing on screen yet; the event below brings the prompt.
      });
    const stop = onPromptPopup((next) => {
      setError(null);
      setShown(next);
    });
    return () => {
      live = false;
      stop();
      freeMenu(menuRef);
    };
  }, []);

  useEffect(
    () =>
      onRecordingState((status) => {
        if (status.phase === "idle") return;
        const now = shownRef.current;
        if (now === null) return;
        setShown(null);
        answerPromptPopup(now.id, "dismiss").catch(() => {
          // Rust hides it on its own timer anyway.
        });
      }),
    [],
  );

  if (shown === null) return null;
  const { id, prompt } = shown;

  const answer = (pressed: PopupAnswer) => {
    // Join opens the meeting and leaves the question up.
    if (pressed !== "join") setShown(null);
    answerPromptPopup(id, pressed).catch((thrown: unknown) => {
      setShown(shown);
      setError(toUiError(thrown).message);
    });
  };

  const openChoices = (event: MouseEvent<HTMLButtonElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    const items = choicesFor(prompt).map((choice) => ({
      id: choice.answer,
      text: choice.text,
      action: () => answer(choice.answer),
    }));
    Menu.new({ items })
      .then((menu) => {
        freeMenu(menuRef);
        menuRef.current = menu;
        // Just under the chevron, from the window's top-left corner.
        return menu.popup(new LogicalPosition(box.left, box.bottom));
      })
      .catch(() => setError("Could not open the menu. The card closes on its own."));
  };

  const join = prompt.canJoin;
  const mainWords = join ? joinWords(prompt.joinService) : "Record";
  const heading = prompt.title ?? prompt.reason;

  return (
    <section
      aria-labelledby="prompt-popup-title"
      aria-live="polite"
      className="flex h-screen items-center gap-5 overflow-hidden rounded-card border-[0.5px] border-rim bg-popup py-4 pr-4 pl-4 text-fg-primary"
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
          onClick={() => answer(join ? "joinAndRecord" : "record")}
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
          onClick={openChoices}
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
