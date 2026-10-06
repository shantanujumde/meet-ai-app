import { render } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { EMPTY_LIVE } from "@/state/transcript";
import { LiveTranscript } from "./LiveTranscript";

/**
 * TUR-110: every live line carries an absolutely positioned `sr-only` label.
 * If the scroller is not positioned, those labels escape it, stretch the
 * page's scroll height and draw clipped glyphs at the pane's right edge.
 *
 * jsdom does no layout, so it cannot measure scrollHeight. The test checks
 * the structure that prevents the bug instead: the scroll box is `relative`,
 * and every `sr-only` span sits inside it.
 */
describe("LiveTranscript scroll containment", () => {
  test("the scroller is positioned and holds every sr-only label", () => {
    const { container } = render(
      <LiveTranscript
        live={{
          ...EMPTY_LIVE,
          status: { state: "running", engine: "apple-speech", detail: null },
          finals: [
            { seq: 1, speaker: "you", start_sec: 1, text: "hello" },
            { seq: 2, speaker: "others", start_sec: 2, text: "hi" },
          ],
        }}
      />,
    );
    const scroller = container.querySelector(".live__scroller");
    expect(scroller).not.toBeNull();
    expect(scroller?.classList.contains("relative")).toBe(true);
    const labels = container.querySelectorAll(".sr-only");
    expect(labels.length).toBeGreaterThan(0);
    for (const label of labels) expect(scroller?.contains(label)).toBe(true);
  });
});
