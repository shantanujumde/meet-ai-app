/**
 * One downloadable model: what it is good for (TUR-79), whether it is on this
 * Mac, the download in flight if there is one, and — once it is here — a
 * radio to make it the whisper model (TUR-75).
 *
 * The main line is plain words from the Rust catalogue. The size and the full
 * id (`small.en-q5_1`) go in the muted detail line, for anyone matching it to
 * `config.jsonc`.
 */

import type { ModelProgress, ModelView, UiError } from "@/ipc/types";
import { formatBytes } from "@/lib/format";
import { Button, Pill, Row, rowDetailVariants } from "@/ui/primitives";
import { Radio } from "@/ui/Radio";
import { ErrorState } from "@/ui/states";
import { DownloadProgress } from "./DownloadProgress";
import { TAG_LABELS } from "./tags";

/** One in-flight state per model, keyed by id. */
export type ModelState = {
  progress?: ModelProgress;
  error?: UiError;
  busy?: boolean;
};

export function ModelRow({
  group,
  model,
  state,
  picked,
  inUse,
  onPick,
  onDownload,
}: {
  /** The model radios' shared `name`. */
  group: string;
  model: ModelView;
  state: ModelState;
  /** This is `transcription.model`. */
  picked: boolean;
  /** …and whisper is the engine recordings use, so it is the one running. */
  inUse: boolean;
  onPick: () => void;
  onDownload: () => void;
}) {
  const { progress, error, busy } = state;
  const downloading = Boolean(busy) && !model.installed;

  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        {/* Only a model on this Mac can be picked: a pick that is not here
            would leave the next whisper recording with nothing to load. One
            that is not here gets no radio rather than a dimmed one, because
            its facts are what the user reads to decide what to download. */}
        {model.installed ? (
          <Radio
            name={group}
            value={model.id}
            checked={picked}
            onChange={onPick}
            className="flex-1"
          >
            <ModelFacts model={model} />
          </Radio>
        ) : (
          <span className="flex min-w-0 flex-1 flex-col gap-1 pl-8">
            <ModelFacts model={model} />
          </span>
        )}
        <div className="flex shrink-0 flex-col items-end gap-2">
          {inUse ? <Pill tone="ok">In use</Pill> : null}
          {model.installed ? (
            <Pill>On this Mac</Pill>
          ) : (
            <Button size="small" disabled={downloading} onClick={onDownload}>
              {downloading ? "Downloading…" : "Download"}
            </Button>
          )}
        </div>
      </div>

      {model.recommended ? (
        <p className="pl-8 text-footnote text-fg-secondary contrast-more:text-fg-primary">
          <span className="font-medium text-accent">Recommended</span> · {model.recommended}
        </p>
      ) : null}

      {downloading ? <DownloadProgress progress={progress} total={model.bytes} /> : null}

      {error ? (
        <ErrorState
          error={error}
          busy={downloading}
          // Both "Try again" and "Download again from scratch" call the same
          // command, and the second one is honest: a checksum failure deletes
          // the bad `.part`, so the next attempt really does start over.
          onRemedy={onDownload}
        />
      ) : null}
    </Row>
  );
}

/** Name, good-for line, tags, then size and id in the muted line. */
function ModelFacts({ model }: { model: ModelView }) {
  return (
    <>
      <span className="text-body font-medium">{model.displayName}</span>
      <span className="text-footnote text-fg-secondary contrast-more:text-fg-primary">
        {model.goodFor}
      </span>
      <TagChips model={model} />
      {/* The size comes from the pinned catalogue, not from a number typed
          into a document — SPEC §2.4's "~1.6 GB" is stale by a factor of
          three. */}
      <span className={rowDetailVariants({ mono: false })}>
        {formatBytes(model.bytes)} · {model.id} · downloaded once, then kept
      </span>
    </>
  );
}

/** The catalogue's tags as small chips, labelled the same on every row. */
function TagChips({ model }: { model: ModelView }) {
  if (model.tags.length === 0) return null;
  return (
    <span className="flex flex-wrap gap-2 py-1">
      {model.tags.map((tag) => (
        <span
          key={tag}
          className="inline-flex h-(--control-h-small) items-center rounded-capsule bg-glass-sunken px-3 text-caption2 text-fg-secondary contrast-more:text-fg-primary"
        >
          {TAG_LABELS[tag]}
        </span>
      ))}
    </span>
  );
}
