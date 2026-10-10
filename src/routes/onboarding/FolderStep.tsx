/**
 * Onboarding step 4: where meetings are written, and the shortcut that
 * records them.
 */

import { ArrowRight, FolderOpen } from "lucide-react";
import { revealMeeting } from "@/ipc/client";
import { osText, shortcutLabel } from "@/lib/osText";
import { useAppStore } from "@/state/app";
import { FolderRow } from "@/ui/FolderRow";
import { IconSquare } from "@/ui/icons";
import { Button, ButtonRow, Card, Pill, Prose } from "@/ui/primitives";

export function FolderStep({ onNext }: { onNext: () => void }) {
  const rootExists = useAppStore((state) => state.meetings?.rootExists ?? false);
  // TUR-165: the button opens the newest meeting's folder, so with no meeting
  // yet it would do nothing. The path is printed in the row either way.
  const hasMeeting = useAppStore((state) => (state.meetings?.meetings.length ?? 0) > 0);

  return (
    <>
      <header className="page__header">
        <h1 className="page__title flex items-center gap-4">
          <IconSquare icon={FolderOpen} />
          Where your meetings live
        </h1>
      </header>
      <Prose>
        Every meeting becomes a folder here, holding the transcript, your notes and the audio. They
        are ordinary markdown files: open them in any editor, search them with Spotlight, keep them
        in a git repo if you want.
      </Prose>
      <Card>
        <FolderRow
          bare
          status={
            !rootExists ? (
              <Pill>Created on first recording</Pill>
            ) : hasMeeting ? (
              <Button size="small" icon={FolderOpen} onClick={() => void revealFirstMeeting()}>
                Show in {osText("fileManager")}
              </Button>
            ) : null
          }
        />
      </Card>
      <Prose>
        Audio is deleted after 7 days unless you change it. The text is kept forever. Press{" "}
        <strong>{shortcutLabel()}</strong> from anywhere to start and stop. This window does not
        need to be open, or even visible.
      </Prose>
      <ButtonRow>
        <Button tone="primary" icon={ArrowRight} onClick={onNext}>
          Continue
        </Button>
      </ButtonRow>
    </>
  );
}

/** Open the newest meeting's folder, which is the quickest route into the root. */
async function revealFirstMeeting() {
  const first = useAppStore.getState().meetings?.meetings[0];
  if (!first) return;
  try {
    await revealMeeting(first.id);
  } catch {
    // Finder refused or the folder moved. The path is printed above either way.
  }
}
