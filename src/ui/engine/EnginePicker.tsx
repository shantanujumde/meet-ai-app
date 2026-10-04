/**
 * The speech engine picker (TUR-75): Automatic, Apple, Whisper or Parakeet
 * (TUR-62), each with one plain line on what it does here.
 *
 * Which choices are greyed out, and the reason under them, come from Rust
 * (`stt::registry::options`, the same decision `auto` uses). Nothing here
 * works out on its own whether an engine can run.
 *
 * Drawn before the probe answers, with every radio held until it does, so
 * nothing below moves when it fills in.
 */

import { Download, Mic } from "lucide-react";
import { type ReactNode, useId } from "react";
import type { EngineChoice, EngineChoices } from "@/ipc/types";
import { formatBytes } from "@/lib/format";
import { IconSquare } from "@/ui/icons";
import { Button, Row, RowValue, rowDetailVariants } from "@/ui/primitives";
import { Radio } from "@/ui/Radio";
import { Checking, ErrorState } from "@/ui/states";
import { DownloadProgress } from "./DownloadProgress";
import type { ModelState } from "./ModelRow";
import { languageNames } from "./tags";

/** What "Automatic" would do on this Mac, in one line. */
export function autoDetail(choices: EngineChoices | null): string {
  if (!choices) return "Picks the best engine for this Mac";
  if (choices.auto === "apple-speech") return "Uses Apple's engine on this Mac";
  if (choices.auto === "whisper") return "Uses Whisper on this Mac, since Apple's engine can't run";
  if (choices.auto === "parakeet") return "Uses Parakeet";
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

/** Parakeet's line (TUR-62): what it is good for, then the one download it needs. */
export function parakeetDetail(choices: EngineChoices | null): string {
  if (!choices) return "Fast on a computer without a graphics card.";
  const model = choices.parakeetModel;
  if (!model.runtimeReady) return model.goodFor;
  const download = model.installed
    ? "Downloaded."
    : `Needs a ${formatBytes(model.bytes)} download, then kept.`;
  return `${model.goodFor} ${download}`;
}

export function EnginePicker({
  choices,
  checking,
  onPick,
  parakeetDownload,
  onDownloadParakeet,
}: {
  choices: EngineChoices | null;
  checking: boolean;
  onPick: (engine: EngineChoice) => void;
  /** The Parakeet model's download in flight, if any (TUR-62). */
  parakeetDownload: ModelState;
  onDownloadParakeet: () => void;
}) {
  const group = useId();
  const ready = choices !== null;
  // No download to offer where ONNX Runtime is missing: it could not run.
  const parakeetMissing =
    choices !== null && choices.parakeetModel.runtimeReady && !choices.parakeetModel.installed;
  const parakeetBusy = Boolean(parakeetDownload.busy) && parakeetMissing;

  return (
    <fieldset className="contents">
      <legend className="sr-only">Speech engine</legend>
      <Row>
        <span className="flex min-w-0 items-center gap-5">
          <IconSquare icon={Mic} />
          <span className="text-body font-semibold">Engine</span>
        </span>
      </Row>
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
      <EngineOption
        group={group}
        value="parakeet"
        name="Parakeet"
        detail={parakeetDetail(choices)}
        reason={choices?.parakeet.available === false ? choices.parakeet.reason : null}
        checked={choices?.engine === "parakeet"}
        disabled={!ready || !choices.parakeet.available}
        onPick={onPick}
        status={
          parakeetMissing ? (
            <Button
              size="small"
              icon={Download}
              disabled={parakeetBusy}
              onClick={onDownloadParakeet}
            >
              {parakeetBusy ? "Downloading…" : "Download"}
            </Button>
          ) : null
        }
      >
        {parakeetBusy && choices ? (
          <DownloadProgress
            progress={parakeetDownload.progress}
            total={choices.parakeetModel.bytes}
          />
        ) : null}
        {parakeetDownload.error ? (
          <ErrorState
            error={parakeetDownload.error}
            busy={parakeetBusy}
            onRemedy={onDownloadParakeet}
          />
        ) : null}
      </EngineOption>
    </fieldset>
  );
}

/** The engine options sit one indent in, past the group's icon square. */
const OPTION_INDENT = "pl-10";

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
  children = null,
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
  /** More under the row, such as a download's progress. */
  children?: ReactNode;
}) {
  return (
    <Row stacked className={OPTION_INDENT}>
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
      {children}
    </Row>
  );
}
