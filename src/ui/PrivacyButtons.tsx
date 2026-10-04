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
 *
 * TUR-51: that is macOS. Windows has one switch, the microphone (loopback
 * needs no permission), and Linux none, so each gets one button: the
 * microphone privacy page on Windows, the desktop's sound settings on Linux.
 */

import { openPrivacySettings } from "@/ipc/client";
import type { PrivacyPane } from "@/ipc/types";
import { currentOs, type Os } from "@/lib/osText";
import { Button } from "./primitives";

const LABEL: Record<PrivacyPane, string> = {
  microphone: "Open Microphone",
  "audio-capture": "Open System Audio Recording",
  calendars: "Open Calendars",
};

/** The one button off macOS, by OS. */
const OTHER_LABEL: Record<Exclude<Os, "macos">, string> = {
  windows: "Open Microphone settings",
  linux: "Open Sound settings",
};

export function PrivacyButtons({ primary }: { primary?: PrivacyPane }) {
  const os = currentOs();
  if (os !== "macos") {
    return (
      <Button
        tone={primary ? "primary" : "neutral"}
        onClick={() => void openSettings("microphone")}
      >
        {OTHER_LABEL[os]}
      </Button>
    );
  }
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
