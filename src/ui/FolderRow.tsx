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

import type { ReactNode } from "react";
import { useChangeFolder } from "@/hooks/useChangeFolder";
import { DEFAULT_ROOT_LABEL } from "@/lib/constants";
import { useAppStore } from "@/state/app";
import { ErrorState } from "@/ui/states";

export function FolderRow({
  status,
  bare = false,
}: {
  /** What sits left of "Change…": whether the folder exists yet, or a way into it. */
  status: ReactNode;
  /**
   * True inside a padded `.card`, where the card's own padding already frames
   * the row; false as one row of a flush card, where the row pads itself.
   */
  bare?: boolean;
}) {
  const root = useAppStore((state) => state.meetings?.root) ?? DEFAULT_ROOT_LABEL;
  const { busy, error, pick } = useChangeFolder();

  return (
    <div
      className="row"
      style={{ flexDirection: "column", alignItems: "stretch", ...(bare ? { padding: 0 } : {}) }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}>
        <span className="row__label">
          <span className="row__name">Meetings folder</span>
          <span className="row__detail">{root}</span>
        </span>
        <span
          className="row__value"
          style={{ display: "flex", alignItems: "center", gap: "var(--space-4)" }}
        >
          {status}
          <button
            type="button"
            className="btn btn--small"
            disabled={busy}
            onClick={() => void pick()}
          >
            {busy ? "Moving…" : "Change…"}
          </button>
        </span>
      </div>
      {error ? <ErrorState error={error} busy={busy} /> : null}
    </div>
  );
}
