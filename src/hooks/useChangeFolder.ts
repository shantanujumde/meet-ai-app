/**
 * The "change the meetings folder" flow, shared between Settings and
 * onboarding so the two screens cannot disagree about what happens: pick a
 * folder, confirm in plain language that every existing meeting moves with
 * it, then move it.
 */

import { ask, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useState } from "react";
import { changeMeetingsFolder, hasBackend } from "@/ipc/client";
import { NO_BACKEND } from "@/ipc/errors";
import { toUiError, type UiError } from "@/ipc/types";
import { DEFAULT_ROOT_LABEL } from "@/lib/constants";
import { useAppStore } from "@/state/app";

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

    const confirmed = await ask(
      `Every existing meeting moves from ${root} to ${picked}. Nothing is deleted — new recordings save there too.`,
      {
        title: "Move your meetings folder?",
        kind: "warning",
        okLabel: "Move meetings",
        cancelLabel: "Cancel",
      },
    );
    if (!confirmed) return;

    setBusy(true);
    try {
      await changeMeetingsFolder(picked);
      await useAppStore.getState().loadMeetings();
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  }, [root]);

  return { busy, error, pick };
}
