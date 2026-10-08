import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import type { RecordingState } from "@/ipc/types";
import { meetingSummary } from "@/test/fixtures";
import { MeetingHeader } from "./MeetingHeader";

function header(recordingState: RecordingState) {
  render(
    <MeetingHeader
      summary={meetingSummary({ recordingState, audioMs: 30 * 60_000 })}
      path="/Users/test/Meetings/2026-10-08-0930-meeting"
      live={null}
      onReveal={() => {}}
      onRename={async () => {}}
      revealError={null}
    />,
  );
  return screen.getByTestId("meeting-meta");
}

/** TUR-145: a meeting a backup stop ended says which on its meta line. */
describe("MeetingHeader after a backup stop", () => {
  test("a recording stopped by sleep says so, in words", () => {
    expect(header("stopped-for-sleep")).toHaveTextContent(
      /30 min\s*·\s*Stopped when this Mac went to sleep/,
    );
  });

  test("a recording stopped after 10 minutes of silence says so", () => {
    expect(header("stopped-for-silence")).toHaveTextContent("Stopped after 10 minutes of silence");
  });

  test("a meeting stopped by hand says nothing about it", () => {
    expect(header("finished")).not.toHaveTextContent(/Stopped/);
  });
});
