/**
 * A model download's progress bar, including the two stretches with nothing
 * to measure: before the first byte, and while the file is being hashed.
 */

import type { ModelProgress } from "@/ipc/types";
import { formatBytes } from "@/lib/format";

export function DownloadProgress({
  progress,
  total,
}: {
  progress: ModelProgress | undefined;
  total: number;
}) {
  // No progress event yet means the request has not gone out. Saying
  // "Starting…" is better than a 0% bar that looks stuck.
  if (!progress) {
    return <Indeterminate label="Starting…" bytes={total} />;
  }

  if (progress.verifying) {
    return (
      <Indeterminate label="Checking the file is the right one…" bytes={progress.totalBytes} />
    );
  }

  const fraction =
    progress.totalBytes > 0
      ? Math.min(1, Math.max(0, progress.downloadedBytes / progress.totalBytes))
      : 0;

  return (
    <div className={BAR}>
      <div
        className={TRACK}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(fraction * 100)}
        aria-label="Download progress"
      >
        {/* The one style left inline: the width is the measurement itself. */}
        <div
          className={`${FILL} transition-[width] duration-(--dur-normal) ease-out`}
          style={{ width: `${fraction * 100}%` }}
        />
      </div>
      <Label
        left={`${formatBytes(progress.downloadedBytes)} of ${formatBytes(progress.totalBytes)}`}
        right={`${Math.round(fraction * 100)}%`}
      />
    </div>
  );
}

const BAR = "flex flex-col gap-3";
const TRACK = "h-[6px] overflow-hidden rounded-capsule bg-glass-sunken";
const FILL = "h-full rounded-capsule bg-accent";

/**
 * Hashing has no sub-progress to report, so the bar sweeps — "working" rather
 * than sitting at 100% looking frozen. Reduce Motion stops the sweep and shows
 * a full, dimmed bar instead.
 */
function Indeterminate({ label, bytes }: { label: string; bytes: number }) {
  return (
    <div className={BAR}>
      <div className={TRACK}>
        <div
          className={`${FILL} w-2/5 animate-progress-sweep motion-reduce:w-full motion-reduce:animate-none motion-reduce:opacity-60`}
        />
      </div>
      <Label left={label} right={formatBytes(bytes)} />
    </div>
  );
}

function Label({ left, right }: { left: string; right: string }) {
  return (
    <p className="flex justify-between gap-5 text-caption1 tabular-nums text-fg-secondary">
      <span>{left}</span>
      <span>{right}</span>
    </p>
  );
}
