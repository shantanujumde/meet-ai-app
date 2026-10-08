/**
 * Settings → Notifications' "Never detect" list (TUR-143): the apps that
 * never ask "Record this call?", however long they use the mic.
 *
 * A prompt's "Never for <App>" adds one; each has a Remove button here.
 * The list is `detection.never_detect` in `config.jsonc`, saved whole
 * through Rust's comment-keeping writer; the call-start loop picks it up on
 * its next reading. Dictation apps, Krisp and the like never ask anyway and
 * are not listed: this is only what the user chose.
 */

import { BellOff } from "lucide-react";
import { useEffect, useState } from "react";
import { neverDetectApps, setNeverDetectApps } from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { Button } from "./primitives";
import { SettingsRow } from "./settings/SettingsSection";

export const NEVER_DETECT_LABEL = "Never detect";

export function NeverDetectList({ onError }: { onError: (error: UiError | null) => void }) {
  const [apps, setApps] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    neverDetectApps()
      .then((saved) => {
        if (live) setApps(saved);
      })
      .catch((thrown: unknown) => {
        if (live) onError(toUiError(thrown));
      });
    return () => {
      live = false;
    };
  }, [onError]);

  const remove = async (app: string) => {
    if (apps === null) return;
    setBusy(true);
    onError(null);
    try {
      setApps(await setNeverDetectApps(apps.filter((kept) => kept !== app)));
    } catch (thrown) {
      onError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  };

  const list = apps ?? [];
  return (
    <SettingsRow
      icon={BellOff}
      name={NEVER_DETECT_LABEL}
      detail={
        list.length === 0
          ? "No apps. Never for, in a call prompt's ⋯ menu, adds one."
          : "These apps never ask to record a call."
      }
    >
      {list.length === 0 ? undefined : (
        <ul aria-label={NEVER_DETECT_LABEL} className="m-0 flex list-none flex-col gap-2 p-0">
          {list.map((app) => (
            <li key={app} className="flex items-center justify-between gap-4">
              <span className="min-w-0 truncate text-body">{app}</span>
              <Button
                size="small"
                disabled={busy}
                aria-label={`Remove ${app} from ${NEVER_DETECT_LABEL}`}
                onClick={() => void remove(app)}
              >
                Remove
              </Button>
            </li>
          ))}
        </ul>
      )}
    </SettingsRow>
  );
}
