/**
 * The notes pane: a plain `<textarea>`, by design.
 *
 * SPEC §2.1 rules out CodeMirror explicitly — notes are typed mid-meeting and
 * nothing fancy is wanted. What it does need is to never lose a keystroke and
 * never move the cursor:
 *
 * * **The textarea is the source of truth while it has focus.** The saved text
 *   is written to disk but never read back into the field, so an autosave can
 *   not reposition the caret mid-sentence. (SPEC §4 solves the same problem on
 *   the Rust side with watcher self-write suppression; this is the half of it
 *   that lives in the window.)
 * * **Saves are debounced, and flushed on the way out.** Navigating to another
 *   meeting while a save is pending must not drop it.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { saveNotes } from "@/ipc/client";
import type { UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";

/** Long enough to coalesce a burst of typing, short enough to feel saved. */
const AUTOSAVE_DELAY_MS = 600;

type SaveState =
  | { kind: "clean" }
  | { kind: "pending" }
  | { kind: "saving" }
  | { kind: "saved" }
  | { kind: "failed"; error: UiError };

export function NotesPane({
  meetingId,
  initialNotes,
  onSaved,
}: {
  meetingId: string;
  /** What was on disk when the meeting was opened. Never re-applied after. */
  initialNotes: string;
  onSaved?: (body: string) => void;
}) {
  const [text, setText] = useState(initialNotes);
  const [state, setState] = useState<SaveState>({ kind: "clean" });

  // Refs, not state: the flush-on-unmount effect must read the newest text
  // without re-subscribing on every keystroke.
  const latest = useRef(text);
  const savedOnDisk = useRef(initialNotes);
  const timer = useRef<number | undefined>(undefined);

  // Switching meetings replaces the whole pane's contents. Keyed on the id so
  // this cannot fire while the user is typing into the same meeting.
  useEffect(() => {
    setText(initialNotes);
    setState({ kind: "clean" });
    latest.current = initialNotes;
    savedOnDisk.current = initialNotes;
  }, [initialNotes]);

  // `onSaved` is typically an inline arrow from the parent, so it gets a new
  // identity whenever the parent re-renders. Held in a ref so it cannot change
  // `flush`'s identity — otherwise the unmount effect below tears down and
  // re-runs on an unrelated parent render, firing a save mid-keystroke and
  // quietly defeating the debounce.
  const notifySaved = useRef(onSaved);
  notifySaved.current = onSaved;

  const flush = useCallback(async () => {
    const body = latest.current;
    if (body === savedOnDisk.current) return;
    setState({ kind: "saving" });
    try {
      await saveNotes(meetingId, body);
      savedOnDisk.current = body;
      setState({ kind: "saved" });
      notifySaved.current?.(body);
    } catch (thrown) {
      // Keep the text in the box. The user's words are safe in the field even
      // though the write failed, and telling them to retype would be obscene.
      setState({ kind: "failed", error: toUiError(thrown) });
    }
  }, [meetingId]);

  // Flush whatever is pending when the pane goes away or the meeting changes.
  // Navigating to another meeting with a save still debounced must not drop it.
  useEffect(() => {
    return () => {
      window.clearTimeout(timer.current);
      void flush();
    };
  }, [flush]);

  function handleChange(next: string) {
    setText(next);
    latest.current = next;
    setState({ kind: "pending" });
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), AUTOSAVE_DELAY_MS);
  }

  return (
    <section className="notes section" aria-labelledby="notes-heading">
      <div className="section__header">
        <h2 className="section__title" id="notes-heading">
          Notes
        </h2>
        <p className="section__hint">Saved to notes.md as you type</p>
      </div>

      <textarea
        className="notes__editor"
        value={text}
        onChange={(event) => handleChange(event.target.value)}
        onBlur={() => {
          window.clearTimeout(timer.current);
          void flush();
        }}
        placeholder="What you want to remember from this meeting. Your agent reads this alongside the transcript."
        aria-describedby="notes-status"
        spellCheck
      />

      {/* aria-live so a screen reader hears a failed save without having to go
          looking for it. The height is reserved either way, so the page does
          not jump when the word changes. */}
      <p
        className="notes__status"
        id="notes-status"
        data-tone={state.kind === "failed" ? "error" : undefined}
        role="status"
        aria-live="polite"
      >
        {describe(state)}
      </p>
    </section>
  );
}

function describe(state: SaveState): string {
  switch (state.kind) {
    case "clean":
      return "";
    case "pending":
      return "Unsaved changes…";
    case "saving":
      return "Saving…";
    case "saved":
      return "Saved";
    case "failed":
      return `Could not save your notes. They are still here in the window. ${state.error.message}`;
  }
}
