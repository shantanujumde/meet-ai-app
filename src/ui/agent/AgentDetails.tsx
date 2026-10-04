/**
 * The picked agent's settings: which model it runs, and where its CLI is.
 *
 * The model is free text with suggestions, because both CLIs accept names
 * meet-ai has never heard of. It saves when the field loses focus or on
 * Enter, not on every key, so typing "sonnet" does not write "s", "so"…
 *
 * Blank is "Default": meet-ai passes no model and the agent picks its own
 * (SPEC A14). The buttons are Default and the agent's first two models; every
 * other model it offers is in the dropdown.
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
import { osText } from "@/lib/osText";
import { Button, Row, RowLabel, RowValue, rowDetailVariants } from "@/ui/primitives";
import { ErrorState } from "@/ui/states";
import { defaultModelText, modelChoices, modelText } from "./agents";

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
  const statusId = useId();
  const [draft, setDraft] = useState(choice.model);

  // A save, or a pick of another agent, replaces what is in the field.
  useEffect(() => setDraft(choice.model), [choice.model]);

  const models = cli?.models ?? [];
  const { chips, more } = modelChoices(models);
  const defaultText = defaultModelText(name, cli);
  const picked = models.find((model) => model.name === choice.model);
  // What the saved model is, in words: the Default sentence, or the list's note.
  const status = choice.model === "" ? defaultText : picked?.note ? modelText(picked) : null;
  const inMore = more.some((model) => model.name === choice.model);

  function choose(model: string) {
    setDraft(model);
    onSave(model);
  }

  return (
    <Row stacked>
      <label htmlFor={id} className="flex flex-col gap-1">
        <span className="text-body font-medium">Model</span>
        <span className={rowDetailVariants({ mono: false })}>
          {`Any model name ${name} accepts. Leave it blank to let ${name} pick.`}
        </span>
      </label>
      <input
        id={id}
        value={draft}
        placeholder={defaultText}
        aria-describedby={status ? statusId : undefined}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => onSave(draft)}
        onKeyDown={(event) => {
          if (event.key === "Enter") onSave(draft);
        }}
        className="w-full rounded-control border-[0.5px] border-separator bg-glass-sunken px-4 py-2 font-mono text-body text-fg-primary placeholder:text-fg-tertiary"
      />
      {status ? (
        <p id={statusId} className={rowDetailVariants({ mono: false })}>
          {status}
        </p>
      ) : null}
      <div className="flex flex-wrap items-center gap-2">
        <span className={rowDetailVariants({ mono: false })}>Suggestions:</span>
        <Button
          size="small"
          tone="quiet"
          aria-pressed={choice.model === ""}
          title={defaultText}
          className="aria-pressed:bg-glass-sunken aria-pressed:text-fg-primary"
          onClick={() => choose("")}
        >
          Default
        </Button>
        {chips.map((model) => (
          <Button
            key={model.name}
            size="small"
            tone="quiet"
            aria-pressed={choice.model === model.name}
            title={modelText(model)}
            className="aria-pressed:bg-glass-sunken aria-pressed:text-fg-primary"
            onClick={() => choose(model.name)}
          >
            {model.label}
          </Button>
        ))}
        {more.length > 0 ? (
          <select
            aria-label="More models"
            value={inMore ? choice.model : ""}
            onChange={(event) => {
              if (event.target.value) choose(event.target.value);
            }}
            className="rounded-control border-[0.5px] border-separator bg-glass-sunken px-3 py-1 text-body text-fg-primary"
          >
            <option value="">More models…</option>
            {more.map((model) => (
              <option key={model.name} value={model.name}>
                {modelText(model)}
              </option>
            ))}
          </select>
        ) : null}
      </div>
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
          If {name} works in {osText("terminal")} but meet-ai cannot find it, choose its file here.
          Apps opened from the Dock do not see the same folders {osText("terminal")} does.
        </p>
      ) : null}
      {error ? <ErrorState error={error} /> : null}
    </Row>
  );
}
