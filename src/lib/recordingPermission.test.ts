import { expect, test } from "vitest";
import type { PermissionStatus, PrivacyPane } from "@/ipc/types";
import { recordingBlocked, systemAudioOnlyOff } from "./recordingPermission";

function status(state: PermissionStatus["state"], denied: PrivacyPane[]): PermissionStatus {
  return { state, measured: true, detail: "test", denied };
}

test("a denied microphone blocks recording", () => {
  expect(recordingBlocked(status("denied", ["microphone"]))).toBe(true);
  expect(recordingBlocked(status("denied", ["microphone", "audio-capture"]))).toBe(true);
  expect(systemAudioOnlyOff(status("denied", ["microphone", "audio-capture"]))).toBe(false);
});

test("only system audio denied records the microphone instead", () => {
  const systemOnly = status("denied", ["audio-capture"]);
  expect(recordingBlocked(systemOnly)).toBe(false);
  expect(systemAudioOnlyOff(systemOnly)).toBe(true);
});

test("a denial naming no switch still blocks, and nothing else does", () => {
  expect(recordingBlocked(status("denied", []))).toBe(true);
  expect(recordingBlocked(status("granted", []))).toBe(false);
  expect(recordingBlocked(status("unknown", []))).toBe(false);
  expect(recordingBlocked(null)).toBe(false);
  expect(systemAudioOnlyOff(null)).toBe(false);
});
