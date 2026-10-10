import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useAppStore } from "@/state/app";
import { meetingSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { FolderStep } from "./FolderStep";

vi.mock("@tauri-apps/plugin-os", () => ({ platform: vi.fn(() => "macos") }));
vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const shortcut = vi.hoisted(() => ({ available: true }));
vi.mock("@/ipc/shortcut", () => ({
  recordShortcutAvailable: () => Promise.resolve(shortcut.available),
}));

const ROOT = "/Users/test/Meetings";

test("an existing but empty folder has no Show in Finder button, which would do nothing", () => {
  // TUR-165: the button reveals the newest meeting, and there is none yet.
  useAppStore.setState({ meetings: { root: ROOT, rootExists: true, meetings: [] } });
  render(<FolderStep onNext={() => {}} />);
  expect(screen.queryByRole("button", { name: /Show in/ })).toBeNull();
  expect(screen.queryByText("Created on first recording")).toBeNull();
  expect(screen.getByText(ROOT)).toBeTruthy();
});

test("a folder with a meeting offers Show in Finder, which opens that meeting", () => {
  const meeting = meetingSummary();
  useAppStore.setState({ meetings: { root: ROOT, rootExists: true, meetings: [meeting] } });
  render(<FolderStep onNext={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "Show in Finder" }));
  expect(ipc.revealMeeting).toHaveBeenCalledWith(meeting.id);
});

test("a folder that does not exist yet says when it will be made", () => {
  useAppStore.setState({ meetings: { root: ROOT, rootExists: false, meetings: [] } });
  render(<FolderStep onNext={() => {}} />);
  expect(screen.getByText("Created on first recording")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Show in/ })).toBeNull();
});

// TUR-170 (from TUR-169): the step names the shortcut only while it is meet-ai's.
test("names the shortcut while meet-ai owns it", async () => {
  shortcut.available = true;
  useAppStore.setState({ meetings: { root: ROOT, rootExists: true, meetings: [] } });
  render(<FolderStep onNext={() => {}} />);
  expect(await screen.findByText(/from anywhere to start and stop/)).toBeTruthy();
});

test("a shortcut another app owns is not advertised", async () => {
  shortcut.available = false;
  useAppStore.setState({ meetings: { root: ROOT, rootExists: true, meetings: [] } });
  render(<FolderStep onNext={() => {}} />);
  await waitFor(() =>
    expect(screen.getByText(/is\s+unavailable because another app/)).toBeTruthy(),
  );
  expect(screen.queryByText(/from anywhere to start and stop/)).toBeNull();
  shortcut.available = true;
});
