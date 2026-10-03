/**
 * The two "Open System Settings" buttons, one per privacy pane.
 *
 * Always both: macOS guards the microphone (`You`) and the system audio tap
 * (`Others`) separately (L5), the two switches live in different panes, and
 * one button can only land on one of them. Before any denial there was a
 * single button and it opened the wrong pane for anyone hunting the
 * Microphone switch.
 *
 * Renders the buttons bare, so they sit in whatever button row the caller
 * already has.
 */

import { openPrivacySettings } from "@/ipc/client";
import type { PrivacyPane } from "@/ipc/types";
import { Button } from "./primitives";

const LABEL: Record<PrivacyPane, string> = {
  microphone: "Open Microphone",
  "audio-capture": "Open System Audio Recording",
  calendars: "Open Calendars",
};

export function PrivacyButtons({ primary }: { primary?: PrivacyPane }) {
  // The pane named `primary` leads and is the tinted one. Without one, the
  // microphone comes first, matching the order the grants are explained in.
  const panes: PrivacyPane[] =
    primary === "audio-capture" ? ["audio-capture", "microphone"] : ["microphone", "audio-capture"];

  return (
    <>
      {panes.map((pane) => (
        <Button
          key={pane}
          tone={pane === primary ? "primary" : "neutral"}
          onClick={() => void openSettings(pane)}
        >
          {LABEL[pane]}
        </Button>
      ))}
    </>
  );
}

/** Open one Settings pane, quietly. The Today pane's denied state uses it too. */
export async function openSettings(pane: PrivacyPane) {
  try {
    await openPrivacySettings(pane);
  } catch {
    // The deep link failed and so did the fallback to the pane root. The
    // copy next to every button names the pane rather than relying on the
    // button, so this is not worth an error screen on top of an error screen.
  }
}
