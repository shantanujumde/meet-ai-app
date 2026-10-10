import { expect, test } from "vitest";
import type { PermissionStatus, PrivacyPane } from "@/ipc/types";
import {
  permissionStepShown,
  recordDisabled,
  recordingBlocked,
  systemAudioOffText,
  systemAudioOnlyOff,
} from "./recordingPermission";

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

test("the permission step is shown per OS", () => {
  const micDenied = status("denied", ["microphone"]);
  expect(permissionStepShown("macos", null)).toBe(true);
  expect(permissionStepShown("macos", status("granted", []))).toBe(true);
  expect(permissionStepShown("windows", micDenied)).toBe(true);
  expect(permissionStepShown("windows", status("granted", []))).toBe(false);
  expect(permissionStepShown("windows", null)).toBe(false);
  expect(permissionStepShown("linux", micDenied)).toBe(false);
});

test("system audio off says what the live recording does, in plain words", () => {
  // TUR-136: the check that finds it runs during the recording, which goes
  // on with the microphone; between recordings it says what the next records.
  const live = systemAudioOffText(true);
  expect(live).toMatch(/^System audio is off: /);
  expect(live).toContain("in System Settings");
  expect(live).toContain("this recording goes on with your microphone only.");
  expect(systemAudioOffText(false)).toContain("recordings capture only your microphone.");
  for (const text of [live, systemAudioOffText(false)]) expect(text).not.toContain("\u2014");
});

test("Record is disabled while denied, busy, starting or stopping (TUR-170)", () => {
  const idle = {
    phase: "idle",
    meetingId: null,
    startedAtMs: null,
    pause: { pausedAtMs: null, pausedTotalMs: 0 },
    error: null,
  } as const;
  const granted = status("granted", []);
  expect(recordDisabled(idle, granted, false)).toBe(false);
  expect(recordDisabled({ ...idle, phase: "recording" }, granted, false)).toBe(false);
  expect(recordDisabled({ ...idle, phase: "starting" }, granted, false)).toBe(true);
  expect(recordDisabled({ ...idle, phase: "stopping" }, granted, false)).toBe(true);
  expect(recordDisabled(idle, granted, true)).toBe(true);
  expect(recordDisabled(idle, status("denied", ["microphone"]), false)).toBe(true);
  expect(recordDisabled(idle, status("denied", ["audio-capture"]), false)).toBe(false);
  expect(recordDisabled(idle, null, false)).toBe(false);
});
