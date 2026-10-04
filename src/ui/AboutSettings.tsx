/**
 * Settings, About (TUR-62): the credits a model licence asks for.
 *
 * Parakeet is NVIDIA's model under CC-BY-4.0, which requires attribution
 * wherever it is used. The sentence and the link come from Rust
 * (`stt::model::parakeet::CREDIT`), the same words as `THIRD_PARTY_NOTICES.md`.
 */

import { invoke } from "@tauri-apps/api/core";
import { type MouseEvent as ReactMouseEvent, useEffect, useState } from "react";
import { modelCredits } from "@/ipc/client";
import type { ModelCredit } from "@/ipc/types";
import { Card, Row } from "@/ui/primitives";

/**
 * Open the link in the user's browser. The webview does not follow a
 * `target="_blank"` link by itself; the opener plugin's default scope allows
 * https URLs.
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
    <section className="section" aria-labelledby="about-heading">
      <h2 className="section__title" id="about-heading">
        About
      </h2>
      <Card flush>
        {credits.map((credit) => (
          <Row stacked key={credit.url}>
            <p className="m-0 text-footnote text-fg-secondary contrast-more:text-fg-primary">
              {credit.text}
            </p>
            <a
              href={credit.url}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => openLink(event, credit.url)}
              className="w-fit text-footnote text-accent"
            >
              {credit.url}
            </a>
          </Row>
        ))}
      </Card>
    </section>
  );
}
