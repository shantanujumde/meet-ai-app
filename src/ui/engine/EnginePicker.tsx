/**
 * The speech engine picker (TUR-75): Automatic, Apple, or Whisper, each with
 * one plain line on what it does here.
 *
 * Which choices are greyed out, and the reason under them, come from Rust
 * (`stt::registry::options`, the same decision `auto` uses). Nothing here
 * works out on its own whether an engine can run.
 *
 * Drawn before the probe answers, with every radio held until it does, so
 * nothing below moves when it fills in.
 */

import { type ReactNode, useId } from "react";
import type { EngineChoice, EngineChoices } from "@/ipc/types";
import { Card, Row, RowValue, rowDetailVariants } from "@/ui/primitives";
import { Radio } from "@/ui/Radio";
import { Checking } from "@/ui/states";
import { languageNames } from "./tags";

/** What "Automatic" would do on this Mac, in one line. */
export function autoDetail(choices: EngineChoices | null): string {
  if (!choices) return "Picks the best engine for this Mac";
  if (choices.auto === "apple-speech") return "Uses Apple's engine on this Mac";
  if (choices.auto === "whisper") return "Uses Whisper on this Mac, since Apple's engine can't run";
  return "Nothing is ready yet: download a model below";
}

/** Apple's line: what it is, then the languages it has installed. */
export function appleDetail(choices: EngineChoices | null): string {
  const about = "Built into macOS 26. Fast, private, no download.";
  const names = languageNames(choices?.languages ?? []);
  const languages =
    names.length > 0 ? names.join(", ") : "whatever macOS supports for on-device dictation";
  return `${about} Languages: ${languages}`;
}

const WHISPER_DETAIL = "Works offline on any Mac. Uses one of the models below.";

export function EnginePicker({
  choices,
  checking,
  onPick,
}: {
  choices: EngineChoices | null;
  checking: boolean;
  onPick: (engine: EngineChoice) => void;
}) {
  const group = useId();
  const ready = choices !== null;

  return (
    <fieldset className="contents">
      <legend className="sr-only">Speech engine</legend>
      <Card flush>
        <EngineOption
          group={group}
          value="auto"
          name="Automatic (recommended)"
          detail={autoDetail(choices)}
          checked={choices?.engine === "auto"}
          disabled={!ready}
          onPick={onPick}
          status={checking ? <Checking label="Checking…" /> : null}
        />
        <EngineOption
          group={group}
          value="apple-speech"
          name="Apple (built in)"
          detail={appleDetail(choices)}
          reason={choices?.apple.available === false ? choices.apple.reason : null}
          checked={choices?.engine === "apple-speech"}
          disabled={!ready || !choices.apple.available}
          onPick={onPick}
        />
        <EngineOption
          group={group}
          value="whisper"
          name="Whisper"
          detail={WHISPER_DETAIL}
          reason={choices?.whisper.available === false ? choices.whisper.reason : null}
          checked={choices?.engine === "whisper"}
          disabled={!ready || !choices.whisper.available}
          onPick={onPick}
        />
      </Card>
    </fieldset>
  );
}

function EngineOption({
  group,
  value,
  name,
  detail,
  reason = null,
  checked,
  disabled,
  onPick,
  status = null,
}: {
  group: string;
  value: EngineChoice;
  name: string;
  detail: string;
  /** Why it cannot be picked, shown under the detail when disabled. */
  reason?: string | null;
  checked: boolean;
  disabled: boolean;
  onPick: (engine: EngineChoice) => void;
  status?: ReactNode;
}) {
  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        {/* flex-1, so the empty space up to the status picks the row too. */}
        <Radio
          name={group}
          value={value}
          checked={checked}
          disabled={disabled}
          onChange={() => onPick(value)}
          className="flex-1"
        >
          <span className="text-body font-medium">{name}</span>
          <span className={rowDetailVariants({ mono: false })}>{detail}</span>
        </Radio>
        {status ? <RowValue className="shrink-0">{status}</RowValue> : null}
      </div>
      {/* Outside the label, so the disabled radio's dimming does not take the
          reason's contrast with it. Lined up under the name. */}
      {reason ? (
        <p className="pl-8 text-footnote text-fg-secondary contrast-more:text-fg-primary">
          {reason}
        </p>
      ) : null}
    </Row>
  );
}
