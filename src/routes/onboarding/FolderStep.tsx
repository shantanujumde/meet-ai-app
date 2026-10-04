/**
 * Onboarding step 4: where meetings are written, and the shortcut that
 * records them.
 */

import { revealMeeting } from "@/ipc/client";
import { SHORTCUT_LABEL } from "@/lib/constants";
import { osText } from "@/lib/osText";
import { useAppStore } from "@/state/app";
import { FolderRow } from "@/ui/FolderRow";
import { Button, ButtonRow, Card, Pill, Prose } from "@/ui/primitives";

export function FolderStep({ onNext }: { onNext: () => void }) {
  const rootExists = useAppStore((state) => state.meetings?.rootExists ?? false);

  return (
    <>
      <header className="page__header">
        <h1 className="page__title">Where your meetings live</h1>
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
            rootExists ? (
              <Button size="small" onClick={() => void revealFirstMeeting()}>
                Show in {osText("fileManager")}
              </Button>
            ) : (
              <Pill>Created on first recording</Pill>
            )
          }
        />
      </Card>
      <Prose>
        Audio is deleted after 7 days by default; the text is kept forever. Press{" "}
        <strong>{SHORTCUT_LABEL}</strong> from anywhere to start and stop — you do not need this
        window open, or even visible.
      </Prose>
      <ButtonRow>
        <Button tone="primary" onClick={onNext}>
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
