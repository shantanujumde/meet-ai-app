/**
 * The "change the meetings folder" flow, shared between Settings and
 * onboarding so the two screens cannot disagree about what happens: pick a
 * folder, confirm in plain language that every existing meeting moves with
 * it, then move it.
 */

import { ask, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useState } from "react";
import { changeMeetingsFolder, hasBackend } from "@/ipc/client";
import { NO_BACKEND, ROOT_POINTER_UNREADABLE } from "@/ipc/errors";
import { toUiError, type UiError } from "@/ipc/types";
import { DEFAULT_ROOT_LABEL } from "@/lib/constants";
import { useAppStore } from "@/state/app";

/** The meetings list failed because meet-ai no longer knows where the folder is. */
function isLostRoot(error: UiError | null): boolean {
  return error?.domain === "app" && error.kind === ROOT_POINTER_UNREADABLE;
}

export function useChangeFolder() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const root = useAppStore((state) => state.meetings?.root) ?? DEFAULT_ROOT_LABEL;

  const pick = useCallback(async () => {
    setError(null);

    // The same `app/no-backend` error every other command rejects with, so the
    // screen shows the agreed copy for it rather than the generic fallback.
    if (!hasBackend()) {
      setError(NO_BACKEND);
      return;
    }

    let picked: string | string[] | null;
    try {
      picked = await open({ directory: true, multiple: false, defaultPath: root });
    } catch (thrown) {
      setError(toUiError(thrown));
      return;
    }
    if (!picked || Array.isArray(picked)) return; // the user cancelled

    // TUR-149: with the pointer to the meetings folder damaged there is no
    // current folder to move from, so the pick only tells meet-ai where the
    // meetings are. Saying they move from the default folder would be wrong.
    const lost = isLostRoot(useAppStore.getState().meetingsError);
    const confirmed = await ask(
      lost
        ? `meet-ai will keep your meetings in ${picked} from now on. Nothing is moved or deleted.`
        : `All your meetings move from ${root} to ${picked}. Nothing is deleted. New recordings save there too.`,
      {
        title: lost ? "Use this meetings folder?" : "Move your meetings folder?",
        kind: "warning",
        okLabel: lost ? "Use this folder" : "Move meetings",
        cancelLabel: "Cancel",
      },
    );
    if (!confirmed) return;

    setBusy(true);
    try {
      await changeMeetingsFolder(picked);
      await useAppStore.getState().loadMeetings({ silent: true });
      // Whether setup was done was unknown while the folder was; it is
      // known again now.
      if (useAppStore.getState().onboarding === null) {
        await useAppStore.getState().loadOnboarding();
      }
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  }, [root]);

  return { busy, error, pick };
}
