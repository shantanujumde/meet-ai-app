import { platform } from "@tauri-apps/plugin-os";
import { render, screen } from "@testing-library/react";
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

function renderStep(status: PermissionStatus) {
  render(<PermissionStep status={status} loading={false} onRecheck={() => {}} onNext={() => {}} />);
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
