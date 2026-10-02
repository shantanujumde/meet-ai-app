/**
 * The picked agent's settings: which model it runs, and where its CLI is.
 *
 * The model is free text with suggestions, because both CLIs accept names
 * meet-ai has never heard of. It saves when the field loses focus or on
 * Enter, not on every key, so typing "sonnet" does not write "s", "so"…
 *
 * The path is for an app opened from Finder that cannot find a CLI which
 * works fine in Terminal (SPEC A11, "Finding the binary").
 */

import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useId, useState } from "react";
import { hasBackend } from "@/ipc/client";
import { NO_BACKEND } from "@/ipc/errors";
import type { AgentChoice, AgentCli, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Button, Row, RowLabel, RowValue, rowDetailVariants } from "@/ui/primitives";
import { ErrorState } from "@/ui/states";

export function ModelField({
  choice,
  name,
  cli,
  onSave,
}: {
  choice: AgentChoice;
  name: string;
  cli: AgentCli | undefined;
  onSave: (model: string) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState(choice.model);

  // A save, or a pick of another agent, replaces what is in the field.
  useEffect(() => setDraft(choice.model), [choice.model]);

  const suggestions = cli?.models ?? [];
  const placeholder = cli?.defaultModel ?? `${name}'s own default`;

  return (
    <Row stacked>
      <label htmlFor={id} className="flex flex-col gap-1">
        <span className="text-body font-medium">Model</span>
        <span className={rowDetailVariants({ mono: false })}>
          {cli?.defaultModel
            ? `Any model name ${name} accepts. Leave it blank for ${cli.defaultModel}.`
            : `Any model name ${name} accepts. Leave it blank to use ${name}'s own default.`}
        </span>
      </label>
      <input
        id={id}
        value={draft}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => onSave(draft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") onSave(draft);
        }}
        className="w-full rounded-control border-[0.5px] border-separator bg-glass-sunken px-4 py-2 font-mono text-body text-fg-primary placeholder:text-fg-tertiary"
      />
      {suggestions.length > 0 ? (
        <div className="flex flex-wrap items-center gap-2">
          <span className={rowDetailVariants({ mono: false })}>Suggestions:</span>
          {suggestions.map((model) => (
            <Button
              key={model}
              size="small"
              tone="quiet"
              aria-pressed={choice.model === model}
              className="aria-pressed:bg-glass-sunken aria-pressed:text-fg-primary"
              onClick={() => {
                setDraft(model);
                onSave(model);
              }}
            >
              {model}
            </Button>
          ))}
        </div>
      ) : null}
    </Row>
  );
}

export function PathField({
  choice,
  name,
  cli,
  onSave,
}: {
  choice: AgentChoice;
  name: string;
  cli: AgentCli | undefined;
  onSave: (binaryPath: string | null) => void;
}) {
  const [error, setError] = useState<UiError | null>(null);

  async function choose() {
    setError(null);
    if (!hasBackend()) {
      setError(NO_BACKEND);
      return;
    }
    let picked: string | string[] | null;
    try {
      picked = await open({ directory: false, multiple: false, title: `Where is ${name}?` });
    } catch (thrown) {
      setError(toUiError(thrown));
      return;
    }
    if (!picked || Array.isArray(picked)) return; // the user cancelled
    onSave(picked);
  }

  const manual = choice.binaryPath !== null;
  const where = choice.binaryPath ?? cli?.path ?? "Not found yet";

  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        <RowLabel
          name={`Where ${name} is`}
          detail={manual ? `${where} (chosen by you)` : `${where} (found automatically)`}
        />
        <RowValue className="flex shrink-0 items-center gap-4">
          {manual ? (
            <Button size="small" tone="quiet" onClick={() => onSave(null)}>
              Find automatically
            </Button>
          ) : null}
          <Button size="small" onClick={() => void choose()}>
            Choose file…
          </Button>
        </RowValue>
      </div>
      {cli?.state === "missing" ? (
        <p className={rowDetailVariants({ mono: false })}>
          If {name} works in Terminal but meet-ai cannot find it, choose its file here. Apps opened
          from the Dock do not see the same folders Terminal does.
        </p>
      ) : null}
      {error ? <ErrorState error={error} /> : null}
    </Row>
  );
}
