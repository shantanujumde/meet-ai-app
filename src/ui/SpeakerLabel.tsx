/**
 * The speaker on a transcript line, shared by the review and live panes.
 *
 * A coloured dot paired with the word, so the speakers stay distinguishable in
 * greyscale and to a colourblind reader (MASTER.md §9). TUR-103: the word
 * replaced a bare initial in a circle, which did not say who was talking.
 * Shown only when the speaker changes, like a chat, so a run of lines from one
 * person reads as one block; a hidden label keeps its column so the text stays
 * aligned. Hidden from VoiceOver: each line carries the full speaker name as
 * visually hidden text instead.
 */

import type { TranscriptLine } from "@/ipc/types";
import { cn } from "@/lib/cn";

type Speaker = TranscriptLine["speaker"];

const DOT: Record<Speaker, string> = {
  You: "bg-speaker-1",
  Others: "bg-speaker-2",
};

export function SpeakerLabel({ speaker, show = true }: { speaker: Speaker; show?: boolean }) {
  return (
    <span
      className="inline-flex items-center gap-2 text-caption1 font-semibold text-fg-secondary"
      data-speaker={speaker}
      aria-hidden="true"
    >
      {show ? (
        <>
          <span className={cn("size-(--speaker-dot) shrink-0 rounded-capsule", DOT[speaker])} />
          {speaker}
        </>
      ) : null}
    </span>
  );
}
