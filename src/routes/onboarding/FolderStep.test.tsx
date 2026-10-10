import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useAppStore } from "@/state/app";
import { meetingSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { FolderStep } from "./FolderStep";

vi.mock("@tauri-apps/plugin-os", () => ({ platform: vi.fn(() => "macos") }));
vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

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
