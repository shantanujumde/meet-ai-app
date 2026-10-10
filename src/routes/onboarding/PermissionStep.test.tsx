import { platform } from "@tauri-apps/plugin-os";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { PermissionStatus } from "@/ipc/types";
import { PermissionStep } from "./PermissionStep";

vi.mock("@tauri-apps/plugin-os", () => ({ platform: vi.fn(() => "macos") }));

const micDenied: PermissionStatus = {
  state: "denied",
  measured: true,
  detail: "test",
  denied: ["microphone"],
};

const systemAudioDenied: PermissionStatus = {
  state: "denied",
  measured: true,
  detail: "test",
  denied: ["audio-capture"],
};

function renderStep(status: PermissionStatus, onNext: () => void = () => {}) {
  render(<PermissionStep status={status} loading={false} onRecheck={() => {}} onNext={onNext} />);
}

afterEach(() => {
  vi.mocked(platform).mockReturnValue("macos");
});

test("macOS names both grants and both panes", () => {
  renderStep(micDenied);
  expect(screen.getByText("Let meet-ai hear your Mac")).toBeTruthy();
  expect(screen.getAllByRole("button", { name: "Open Microphone" }).length).toBeGreaterThan(0);
  expect(screen.getByRole("button", { name: "Open System Audio Recording" })).toBeTruthy();
});

test("Windows names only the microphone and its Settings page", () => {
  vi.mocked(platform).mockReturnValue("windows");
  renderStep(micDenied);
  expect(screen.getByText("Windows is blocking the microphone")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Open Microphone settings" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Open System Audio Recording" })).toBeNull();
  expect(screen.queryByText(/System Settings/)).toBeNull();
});

test("macOS with only System Audio off still offers Continue, beside the fix-it steps", () => {
  // TUR-165: the microphone still records (TUR-87), so setup must not end here.
  const onNext = vi.fn();
  renderStep(systemAudioDenied, onNext);
  expect(screen.getByRole("alert")).toHaveTextContent(/System Audio Recording Only/);
  expect(screen.getByRole("button", { name: "Check again" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(onNext).toHaveBeenCalledTimes(1);
});

test("macOS with the microphone denied offers no Continue", () => {
  renderStep(micDenied);
  expect(screen.getByText("meet-ai is not allowed to record audio")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Continue" })).toBeNull();
});
