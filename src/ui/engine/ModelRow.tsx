/**
 * One downloadable model: its size, whether it is on this Mac, and the
 * download in flight if there is one.
 */

import type { ModelProgress, ModelView, UiError } from "@/ipc/types";
import { formatBytes } from "@/lib/format";
import { Button, Pill, Row, RowLabel } from "@/ui/primitives";
import { ErrorState } from "@/ui/states";
import { DownloadProgress } from "./DownloadProgress";

/** One in-flight state per model, keyed by id. */
export type ModelState = {
  progress?: ModelProgress;
  error?: UiError;
  busy?: boolean;
};

export function ModelRow({
  model,
  state,
  onDownload,
}: {
  model: ModelView;
  state: ModelState;
  onDownload: () => void;
}) {
  const { progress, error, busy } = state;
  const downloading = Boolean(busy) && !model.installed;

  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        {/* The size comes from the pinned catalogue, not from a number typed
            into a document — SPEC §2.4's "~1.6 GB" is stale by a factor of
            three. */}
        <RowLabel
          name={model.id}
          detail={`${formatBytes(model.bytes)} · downloaded once, then kept`}
          mono={false}
        />
        {model.installed ? (
          <Pill tone="ok">On this Mac</Pill>
        ) : (
          <Button size="small" disabled={downloading} onClick={onDownload}>
            {downloading ? "Downloading…" : "Download"}
          </Button>
        )}
      </div>

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
