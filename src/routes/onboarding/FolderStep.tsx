/**
 * Onboarding step 4: where meetings are written, and the shortcut that
 * records them.
 */

import { revealMeeting } from "@/ipc/client";
import { SHORTCUT_LABEL } from "@/lib/constants";
import { useAppStore } from "@/state/app";
import { FolderRow } from "@/ui/FolderRow";

export function FolderStep({ onFinish }: { onFinish: () => void }) {
  const rootExists = useAppStore((state) => state.meetings?.rootExists ?? false);

  return (
    <>
      <header className="page__header">
        <h1 className="page__title">Where your meetings live</h1>
      </header>
      <p className="prose">
        Every meeting becomes a folder here, holding the transcript, your notes and the audio. They
        are ordinary markdown files: open them in any editor, search them with Spotlight, keep them
        in a git repo if you want.
      </p>
      <div className="card">
        <FolderRow
          bare
          status={
            rootExists ? (
              <button
                type="button"
                className="btn btn--small"
                onClick={() => void revealFirstMeeting()}
              >
                Show in Finder
              </button>
            ) : (
              <span className="badge">Created on first recording</span>
            )
          }
        />
      </div>
      <p className="prose">
        Audio is deleted after 7 days by default; the text is kept forever. Press{" "}
        <strong>{SHORTCUT_LABEL}</strong> from anywhere to start and stop — you do not need this
        window open, or even visible.
      </p>
      <div className="btn-row">
        <button type="button" className="btn btn--primary" onClick={onFinish}>
          Done
        </button>
      </div>
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
