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
    return (
      <div className="progress">
        <div className="progress__track">
          <div className="progress__fill progress__fill--indeterminate" />
        </div>
        <p className="progress__label">
          <span>Starting…</span>
          <span>{formatBytes(total)}</span>
        </p>
      </div>
    );
  }

  if (progress.verifying) {
    return (
      <div className="progress">
        <div className="progress__track">
          <div className="progress__fill progress__fill--indeterminate" />
        </div>
        <p className="progress__label">
          <span>Checking the file is the right one…</span>
          <span>{formatBytes(progress.totalBytes)}</span>
        </p>
      </div>
    );
  }

  const fraction =
    progress.totalBytes > 0
      ? Math.min(1, Math.max(0, progress.downloadedBytes / progress.totalBytes))
      : 0;

  return (
    <div className="progress">
      <div
        className="progress__track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(fraction * 100)}
        aria-label="Download progress"
      >
        <div className="progress__fill" style={{ width: `${fraction * 100}%` }} />
      </div>
      <p className="progress__label">
        <span>
          {formatBytes(progress.downloadedBytes)} of {formatBytes(progress.totalBytes)}
        </span>
        <span>{Math.round(fraction * 100)}%</span>
      </p>
    </div>
  );
}
