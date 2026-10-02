/**
 * A button that asks Rust for a prompt and puts it on the clipboard.
 *
 * Used for Start Work on a ticket (L14) and for a meeting's Copy prompt when no
 * agent is set up (A11). The prompt text is always Rust's: the window never
 * writes one, it only carries it to the clipboard.
 *
 * Two different things can go wrong, and they are shown differently:
 *
 * * **Rust could not make the prompt** — a template with a mistake in it, a
 *   ticket that has gone. That is a real error, shown with `ErrorState`.
 * * **The clipboard refused** — not the user's fault and not worth an error
 *   screen. The prompt is already in hand, so it is shown, selected, for the
 *   user to copy themselves.
 */

import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { cn } from "@/lib/cn";
import { COPIED_RESET_MS } from "@/lib/constants";
import { Button, ButtonRow, rowDetailVariants } from "./primitives";
import { ErrorState } from "./states";

type Outcome =
  | { kind: "idle" }
  | { kind: "busy" }
  | { kind: "copied" }
  | { kind: "render-failed"; error: UiError }
  | { kind: "refused"; prompt: string };

const REFUSED_MESSAGE =
  "Couldn't copy to the clipboard. Select the prompt below and copy it yourself.";

export function CopyPromptButton({
  label,
  render,
  hint,
  size,
  tone,
  className,
}: {
  /** What the button says, e.g. "Start Work". It keeps saying it while busy. */
  label: string;
  /** Asks Rust for the prompt. A rejection is shown as a `UiError`. */
  render: () => Promise<string>;
  /** A short line beside the button explaining what the prompt is for. */
  hint?: ReactNode;
  size?: "regular" | "small";
  tone?: "neutral" | "primary" | "quiet";
  className?: string;
}) {
  const [outcome, setOutcome] = useState<Outcome>({ kind: "idle" });
  const resetTimer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  async function handleClick() {
    window.clearTimeout(resetTimer.current);
    setOutcome({ kind: "busy" });

    let prompt: string;
    try {
      prompt = await render();
    } catch (caught) {
      setOutcome({ kind: "render-failed", error: toUiError(caught) });
      return;
    }

    try {
      await copyText(prompt);
    } catch {
      setOutcome({ kind: "refused", prompt });
      return;
    }

    setOutcome({ kind: "copied" });
    resetTimer.current = window.setTimeout(() => setOutcome({ kind: "idle" }), COPIED_RESET_MS);
  }

  const copied = outcome.kind === "copied";

  return (
    <div className={cn("flex flex-col gap-4", className)}>
      <ButtonRow>
        <Button
          size={size}
          tone={tone}
          disabled={outcome.kind === "busy"}
          onClick={() => void handleClick()}
        >
          {copied ? "Copied" : label}
        </Button>
        {hint ? <span className={rowDetailVariants({ mono: false })}>{hint}</span> : null}
        {/* Heard, not seen: the button's own "Copied" is the visible half. */}
        <span className="sr-only" role="status" aria-live="polite">
          {copied ? "Prompt copied to the clipboard" : ""}
        </span>
      </ButtonRow>
      {outcome.kind === "render-failed" ? <ErrorState error={outcome.error} /> : null}
      {outcome.kind === "refused" ? <CopyByHand prompt={outcome.prompt} /> : null}
    </div>
  );
}

/** The prompt, selected and read-only, for when the clipboard said no. */
function CopyByHand({ prompt }: { prompt: string }) {
  const field = useRef<HTMLTextAreaElement>(null);

  // Layout, not passive: the field is focused in the same commit that shows
  // it, so nothing can see it on screen and not yet selected.
  useLayoutEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);

  return (
    <div className="flex flex-col gap-2">
      <p className="text-callout text-fg-secondary">{REFUSED_MESSAGE}</p>
      <textarea
        ref={field}
        readOnly
        aria-label="Prompt to copy"
        rows={8}
        value={prompt}
        onFocus={(event) => event.currentTarget.select()}
        className="w-full rounded-control border-[0.5px] border-separator bg-glass-sunken px-4 py-3 font-mono text-caption1 text-fg-primary"
      />
    </div>
  );
}
