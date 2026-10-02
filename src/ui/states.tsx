/**
 * The empty, loading and error states, as components rather than as
 * afterthoughts scattered through the screens.
 *
 * Having them here is what makes the interface bar checkable: there is one
 * place to look to confirm that every state has real copy, and a screen that
 * skipped one is visible by its absence.
 */

import type { ReactNode } from "react";
import { useState } from "react";
import { copyFor, detailsFor } from "@/ipc/errors";
import type { UiError } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { COPIED_RESET_MS } from "@/lib/constants";
import { Button, ButtonRow } from "./primitives";

/**
 * A designed empty state: what is not here, and what to do about it.
 *
 * `body` is required on purpose. An empty state with a title and no next step
 * is the thing this component exists to prevent.
 */
export function EmptyState({
  title,
  body,
  action,
  centered = false,
}: {
  title: string;
  body: string;
  action?: ReactNode;
  centered?: boolean;
}) {
  return (
    <div className={centered ? "state state--center" : "state"}>
      <h2 className="state__title">{title}</h2>
      <p className="state__body">{body}</p>
      {action}
    </div>
  );
}

/**
 * An inline "still working" row.
 *
 * Sized to match the content that replaces it, so the page does not jump when
 * the answer arrives. This is the row the settings screen shows while
 * `meet-stt --probe` runs.
 */
export function Checking({ label }: { label: string }) {
  return (
    <span className="checking" role="status">
      <span className="checking__dot" aria-hidden="true" />
      {label}
    </span>
  );
}

/**
 * An error with a human sentence, the verbatim Rust message, and the button
 * the agreed mapping says to offer.
 *
 * `onRemedy` receives the remedy from the table so the screen decides how to
 * carry it out — retrying a download and opening System Settings are not this
 * component's business. "Copy details" is handled here because it is the same
 * everywhere.
 */
export function ErrorState({
  error,
  onRemedy,
  busy = false,
}: {
  error: UiError;
  onRemedy?: (remedy: ReturnType<typeof copyFor>["remedy"]) => void;
  busy?: boolean;
}) {
  const copy = copyFor(error);
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    const details = detailsFor(error);
    try {
      await copyText(details);
      setCopied(true);
      window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch {
      // Clipboard access can be refused. The message is on screen and
      // selectable either way, so this is not worth a second error on top of
      // the first one.
      setCopied(false);
    }
  }

  const isCopy = copy.remedy.action === "copy-details";
  const showButton = copy.actionLabel !== null && (isCopy || onRemedy !== undefined);

  return (
    <div className={copy.security ? "state state--security" : "state state--error"} role="alert">
      <h2 className="state__title">{copy.headline}</h2>
      <p className="state__body">{copy.body}</p>
      {/* The Rust error's own sentence, verbatim. Never the only thing shown,
          and never parsed to decide what to show. */}
      <p className="state__detail">{error.message}</p>
      {showButton ? (
        <ButtonRow>
          <Button
            disabled={busy}
            onClick={() => (isCopy ? void handleCopy() : onRemedy?.(copy.remedy))}
          >
            {isCopy && copied ? "Copied" : copy.actionLabel}
          </Button>
        </ButtonRow>
      ) : null}
    </div>
  );
}
