/**
 * Settings, About (TUR-62): the credits a model licence asks for.
 *
 * Parakeet is NVIDIA's model under CC-BY-4.0, which requires attribution
 * wherever it is used. The sentence and the link come from Rust
 * (`stt::model::parakeet::CREDIT`), the same words as `THIRD_PARTY_NOTICES.md`.
 */

import { invoke } from "@tauri-apps/api/core";
import { Info } from "lucide-react";
import { type MouseEvent as ReactMouseEvent, useEffect, useState } from "react";
import { modelCredits } from "@/ipc/client";
import type { ModelCredit } from "@/ipc/types";
import { RowLabel, rowVariants } from "@/ui/primitives";
import { SettingsSection } from "@/ui/settings/SettingsSection";

/**
 * Open the link in the user's browser. The webview does not follow a
 * `target="_blank"` link by itself. The main window's capability allows
 * opening only this link's address (TUR-158).
 */
function openLink(event: ReactMouseEvent<HTMLAnchorElement>, url: string) {
  event.preventDefault();
  invoke("plugin:opener|open_url", { url }).catch(() => {});
}

export function AboutSettings() {
  const [credits, setCredits] = useState<ModelCredit[]>([]);

  useEffect(() => {
    // Constant data; if it cannot be read there is simply nothing to list.
    modelCredits()
      .then(setCredits)
      .catch(() => setCredits([]));
  }, []);

  if (credits.length === 0) return null;

  return (
    <SettingsSection title="About">
      {credits.map((credit) => (
        <div className={rowVariants()} key={credit.url}>
          <RowLabel
            icon={Info}
            name="Credits"
            mono={false}
            detail={
              <>
                {credit.text}{" "}
                <a
                  href={credit.url}
                  target="_blank"
                  rel="noreferrer"
                  onClick={(event) => openLink(event, credit.url)}
                  className="text-accent-text underline-offset-2 hover:underline"
                >
                  {credit.url}
                </a>
              </>
            }
          />
        </div>
      ))}
    </SettingsSection>
  );
}
