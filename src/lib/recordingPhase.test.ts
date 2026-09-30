import { expect, test } from "vitest";
import type { RecordingPhase } from "@/ipc/types";
import { changesMeetingList } from "./recordingPhase";

const PHASES: RecordingPhase[] = ["idle", "starting", "recording", "stopping"];

test("only a recording that begins or one that finishes refreshes the list", () => {
  const refreshing = PHASES.flatMap((from) =>
    PHASES.filter((to) => changesMeetingList(from, to)).map((to) => `${from}→${to}`),
  );
  expect(refreshing.sort()).toEqual(
    ["idle→recording", "recording→idle", "starting→recording", "stopping→idle"].sort(),
  );
});

test("a start Rust refused before recording does not refresh the list", () => {
  // `idle → starting → idle`: no folder was made, so there is nothing to re-read.
  expect(changesMeetingList("idle", "starting")).toBe(false);
  expect(changesMeetingList("starting", "idle")).toBe(false);
});
