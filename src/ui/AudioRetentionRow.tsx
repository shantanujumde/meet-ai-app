/**
 * Settings → Files: how long recorded audio is kept (SPEC L16, TUR-45).
 *
 * Read-only: `audio.retention_days` is changed in `config.jsonc`. Transcripts,
 * notes and tasks are never deleted, so the line is only about audio.
 */

import { useEffect, useState } from "react";
import { audioRetentionDays } from "@/ipc/client";
import { Row, RowLabel } from "./primitives";

/** What `retention_days` means, in one sentence. */
export function retentionSentence(days: number): string {
  if (days < 0) return "Audio is kept forever";
  if (days === 0) return "Audio is deleted once the transcript is done";
  return `Audio is kept for ${days} ${days === 1 ? "day" : "days"}`;
}

export function AudioRetentionRow() {
  const [days, setDays] = useState<number | null>(null);

  useEffect(() => {
    let live = true;
    audioRetentionDays()
      .then((value) => {
        if (live) setDays(value);
      })
      // A read that failed says nothing rather than something untrue.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  if (days === null) return null;
  return (
    <Row>
      <RowLabel
        name="Audio"
        detail={`${retentionSentence(days)}. Transcripts and notes are always kept.`}
        mono={false}
      />
    </Row>
  );
}
