/**
 * The meeting's title, renamed in place (TUR-103).
 *
 * The calendar names a meeting while it records and the agent's notes may
 * rename it after; the user can rename it here at any time, and that name is
 * kept from then on (`store::meeting_title`).
 *
 * The title reads as the page's big heading. It is a button inside the
 * `<h1>`, named "Rename meeting": a click or Enter swaps it for a text field.
 * In the field Enter saves, Escape puts the old title back, and clicking away
 * saves. A blank field saves nothing and keeps the old title. The heading
 * keeps the title as its name, so VoiceOver's heading list still reads it.
 */

import { Pencil } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Icon } from "./icons";
import { ErrorState } from "./states";

export const RENAME_LABEL = "Rename meeting";

/** Matches `store::meeting_title::MAX_TITLE_CHARS`, so the field stops where Rust would cut. */
export const MAX_TITLE_CHARS = 80;

/** Padding the button and the field share, pulled back out so the words line up with the meta line. */
const INSET = "-mx-2 px-2 rounded-control";

export function MeetingTitle({
  title,
  onRename,
}: {
  title: string;
  /** Saves the new title; rejects with the reason it could not. */
  onRename: (title: string) => Promise<void>;
}) {
  // The words in the field while it is open, else null.
  const [draft, setDraft] = useState<string | null>(null);
  // The title on its way to disk, shown in place of the old one meanwhile.
  const [saving, setSaving] = useState<string | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  const field = useRef<HTMLInputElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  // Set once the field has been saved or cancelled, so the blur that follows
  // Enter or Escape does not save it a second time.
  const closed = useRef(false);
  // Enter and Escape hand focus back to the title; clicking away does not.
  const refocus = useRef(false);

  const editing = draft !== null;
  useEffect(() => {
    if (editing) {
      field.current?.focus();
      field.current?.select();
    } else if (refocus.current) {
      refocus.current = false;
      button.current?.focus();
    }
  }, [editing]);

  function open() {
    // Not disabled while saving: a disabled button would drop the focus
    // Enter just handed back to it.
    if (saving !== null) return;
    closed.current = false;
    setError(null);
    setDraft(title);
  }

  function cancel() {
    closed.current = true;
    refocus.current = true;
    setDraft(null);
  }

  async function save(next: string) {
    if (closed.current) return;
    closed.current = true;
    setDraft(null);
    const trimmed = next.trim();
    if (trimmed === "" || trimmed === title) return;
    setSaving(trimmed);
    try {
      await onRename(trimmed);
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setSaving(null);
    }
  }

  return (
    <>
      {editing ? (
        <input
          ref={field}
          aria-label="Meeting title"
          className={cn(
            "page__title w-full min-w-0 border-[0.5px] border-separator bg-glass-sunken text-fg-primary",
            INSET,
          )}
          value={draft}
          maxLength={MAX_TITLE_CHARS}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={(event) => void save(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              refocus.current = true;
              void save(event.currentTarget.value);
            } else if (event.key === "Escape") {
              event.preventDefault();
              cancel();
            }
          }}
        />
      ) : (
        <h1 className="page__title wrap-anywhere" aria-label={saving ?? title}>
          <button
            ref={button}
            type="button"
            aria-label={RENAME_LABEL}
            title={RENAME_LABEL}
            aria-busy={saving !== null}
            className={cn(
              "group inline-flex max-w-full cursor-text items-center gap-3 border-[0.5px] border-transparent text-left",
              "[transition:background-color_var(--dur-fast)_var(--ease-out)] hover:bg-control",
              INSET,
            )}
            onClick={open}
          >
            <span className="min-w-0 wrap-anywhere">{saving ?? title}</span>
            <Icon
              icon={Pencil}
              className="text-fg-tertiary opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100"
            />
          </button>
        </h1>
      )}
      {error ? <ErrorState error={error} /> : null}
    </>
  );
}
