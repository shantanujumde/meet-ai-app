/**
 * The (i) button on a whisper model row and the language list it opens
 * (TUR-94).
 *
 * The list comes from the Rust catalogue (`crates/stt/src/languages.rs`,
 * taken from OpenAI's `tokenizer.py`), so the screen never decides which
 * languages a model knows. An inline disclosure rather than a floating
 * popover: the app has no popover primitive, and a panel inside the row needs
 * no positioning code. Escape or a click outside closes it and puts focus back
 * on the button.
 */

import { invoke } from "@tauri-apps/api/core";
import { type MouseEvent as ReactMouseEvent, type RefObject, useEffect } from "react";

export const ACCURACY_NOTE =
  "Accuracy varies a lot by language. It is best for English and widely spoken languages, weaker for less common ones.";
export const LANGUAGES_SOURCE_URL =
  "https://github.com/openai/whisper#available-models-and-languages";

/**
 * Open the source in the user's browser. The webview does not follow a
 * `target="_blank"` link by itself. The main window's capability allows
 * opening only this link's address (TUR-158).
 */
function openSource(event: ReactMouseEvent<HTMLAnchorElement>) {
  event.preventDefault();
  invoke("plugin:opener|open_url", { url: LANGUAGES_SOURCE_URL }).catch(() => {});
}

/** "100 languages", or "English only" for a one-language model. */
export function languagesTitle(languages: string[]): string {
  if (languages.length === 1 && languages[0] === "English") return "English only";
  return `${languages.length} ${languages.length === 1 ? "language" : "languages"}`;
}

export function LanguagesButton({
  modelName,
  open,
  panelId,
  buttonRef,
  onToggle,
}: {
  modelName: string;
  open: boolean;
  panelId: string;
  buttonRef: RefObject<HTMLButtonElement | null>;
  onToggle: () => void;
}) {
  return (
    <button
      ref={buttonRef}
      type="button"
      aria-label={`Supported languages for ${modelName}`}
      aria-expanded={open}
      aria-controls={panelId}
      onClick={onToggle}
      className="inline-flex size-(--control-h-small) shrink-0 cursor-pointer items-center justify-center rounded-full border-0 bg-transparent p-0 text-fg-secondary hover:bg-glass-sunken hover:text-fg-primary contrast-more:text-fg-primary"
    >
      <svg aria-hidden="true" viewBox="0 0 16 16" className="size-6" fill="none">
        <circle cx="8" cy="8" r="6.5" stroke="currentColor" strokeWidth="1.25" />
        <path d="M8 7.25v4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
        <circle cx="8" cy="4.9" r="0.9" fill="currentColor" />
      </svg>
    </button>
  );
}

export function LanguagesPanel({
  id,
  languages,
  modelName,
  buttonRef,
  panelRef,
  onClose,
}: {
  id: string;
  languages: string[];
  modelName: string;
  buttonRef: RefObject<HTMLButtonElement | null>;
  panelRef: RefObject<HTMLElement | null>;
  onClose: () => void;
}) {
  useEffect(() => {
    const close = () => {
      onClose();
      buttonRef.current?.focus();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    const onPointer = (event: PointerEvent | MouseEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (panelRef.current?.contains(target) || buttonRef.current?.contains(target)) return;
      close();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [onClose, buttonRef, panelRef]);

  const sorted = [...languages].sort((a, b) => a.localeCompare(b, "en"));
  const title = languagesTitle(languages);

  return (
    <section
      id={id}
      ref={panelRef}
      aria-label={`Supported languages for ${modelName}`}
      className="ml-8 flex flex-col gap-2 rounded-control bg-glass-sunken p-4 contrast-more:border contrast-more:border-fg-primary"
    >
      <h4 className="m-0 text-footnote font-medium text-fg-primary">{title}</h4>
      <ul className="m-0 grid max-h-48 list-none grid-cols-2 gap-x-4 gap-y-1 overflow-y-auto p-0 text-footnote text-fg-secondary contrast-more:text-fg-primary sm:grid-cols-3">
        {sorted.map((name) => (
          <li key={name}>{name}</li>
        ))}
      </ul>
      <p className="m-0 text-caption1 text-fg-secondary contrast-more:text-fg-primary">
        {ACCURACY_NOTE}
      </p>
      <a
        href={LANGUAGES_SOURCE_URL}
        target="_blank"
        rel="noreferrer"
        onClick={openSource}
        className="text-caption1 text-accent"
      >
        Source: OpenAI Whisper
      </a>
    </section>
  );
}
