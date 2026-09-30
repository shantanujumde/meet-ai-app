/**
 * The speaker chip on a transcript line, shared by the review and live panes.
 *
 * Colour paired with an initial, so the speakers stay distinguishable in
 * greyscale and to a colourblind reader (MASTER.md §9). Hidden from VoiceOver:
 * each line carries the full speaker name as visually hidden text instead.
 */

import type { TranscriptLine } from "@/ipc/types";
import { cn } from "@/lib/cn";

type Speaker = TranscriptLine["speaker"];

const FILL: Record<Speaker, string> = {
  You: "bg-speaker-1",
  Others: "bg-speaker-2",
};

export function SpeakerLabel({ speaker }: { speaker: Speaker }) {
  return (
    <span
      className={cn(
        "grid size-[18px] place-items-center self-center rounded-capsule",
        "text-caption2 font-semibold text-on-accent",
        FILL[speaker],
      )}
      data-speaker={speaker}
      aria-hidden="true"
    >
      {speaker === "You" ? "Y" : "O"}
    </span>
  );
}
