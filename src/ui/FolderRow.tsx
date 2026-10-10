/**
 * The meetings-folder row: where meetings are written, and the button that
 * moves them.
 *
 * Settings and onboarding's last step both show it. One component, with the
 * folder flow from {@link useChangeFolder} inside it, so the two screens
 * cannot drift on the label, the busy wording or where the error lands. Only
 * the right-hand status differs — Settings states a fact, onboarding offers
 * Finder — so that is the one thing a caller passes in.
 */

import { Folder, FolderOpen } from "lucide-react";
import type { ReactNode } from "react";
import { useChangeFolder } from "@/hooks/useChangeFolder";
import { DEFAULT_ROOT_LABEL } from "@/lib/constants";
import { useAppStore } from "@/state/app";
import { Button, RowValue } from "@/ui/primitives";
import { SettingsRow } from "@/ui/settings/SettingsSection";
import { ErrorState } from "@/ui/states";

export function FolderRow({
  status,
  bare = false,
}: {
  /** What sits left of "Change…": whether the folder exists yet, or a way into it. */
  status: ReactNode;
  /**
   * True inside a padded card, where the card's own padding already frames
   * the row; false as one row of a flush card, where the row pads itself.
   */
  bare?: boolean;
}) {
  const root = useAppStore((state) => state.meetings?.root) ?? DEFAULT_ROOT_LABEL;
  const { busy, error, pick } = useChangeFolder();

  return (
    <SettingsRow
      icon={Folder}
      name="Meetings folder"
      detail={root}
      mono
      bare={bare}
      control={
        <RowValue className="flex items-center gap-4">
          {status}
          <Button size="small" icon={FolderOpen} disabled={busy} onClick={() => void pick()}>
            {busy ? "Moving…" : "Change…"}
          </Button>
        </RowValue>
      }
    >
      {error ? <ErrorState error={error} busy={busy} /> : undefined}
    </SettingsRow>
  );
}
