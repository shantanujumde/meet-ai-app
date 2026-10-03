/**
 * Settings → Files: how long recorded audio is kept (SPEC L16, TUR-45).
 *
 * Read-only: `audio.retention_days` is changed in `config.jsonc`. Transcripts,
 * notes and tasks are never deleted, so the line is only about audio. When
 * the config cannot be trusted the job deletes nothing, and the line says so
 * rather than showing a default (TUR-85).
 */

import { useEffect, useState } from "react";
import { type AudioRetention, audioRetentionDays } from "@/ipc/client";
import { Row, RowLabel } from "./primitives";

/** The line while the retention job is paused (TUR-85). */
export const RETENTION_PAUSED = "Audio cleanup paused: config.jsonc could not be read";

/** What `retention_days` means, in one sentence. */
export function retentionSentence(days: number): string {
  if (days < 0) return "Audio is kept forever";
  if (days === 0) return "Audio is deleted once the transcript is done";
  return `Audio is kept for ${days} ${days === 1 ? "day" : "days"}`;
}

/** The whole line: the retention sentence, or why it is paused. */
export function retentionDetail(retention: AudioRetention): string {
  if (retention.state === "paused") {
    return `${RETENTION_PAUSED} (${retention.reason}). No audio is deleted until it is fixed.`;
  }
  return `${retentionSentence(retention.days)}. Transcripts and notes are always kept.`;
}

export function AudioRetentionRow() {
  const [retention, setRetention] = useState<AudioRetention | null>(null);

  useEffect(() => {
    let live = true;
    audioRetentionDays()
      .then((value) => {
        if (live) setRetention(value);
      })
      // A read that failed says nothing rather than something untrue.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  if (retention === null) return null;
  return (
    <Row>
      <RowLabel name="Audio" detail={retentionDetail(retention)} mono={false} />
    </Row>
  );
}
