/**
 * The speaker chip on a transcript line, shared by the review and live panes.
 *
 * Colour paired with an initial, so the speakers stay distinguishable in
 * greyscale and to a colourblind reader (MASTER.md §9). Hidden from VoiceOver:
 * each line carries the full speaker name as visually hidden text instead.
 */

import type { TranscriptLine } from "@/ipc/types";

export function SpeakerLabel({ speaker }: { speaker: TranscriptLine["speaker"] }) {
  return (
    <span className="transcript__speaker" data-speaker={speaker} aria-hidden="true">
      {speaker === "You" ? "Y" : "O"}
    </span>
  );
}
