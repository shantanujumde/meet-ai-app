import { act, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { useAppStore } from "@/state/app";
import { useRecordingStore } from "@/state/recording";
import { meetingSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { Meetings } from "./Meetings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const shortcut = vi.hoisted(() => ({ available: true }));
vi.mock("@/ipc/shortcut", () => ({
  recordShortcutAvailable: () => Promise.resolve(shortcut.available),
}));

async function renderEmpty() {
  ipc.listMeetings.mockResolvedValue({
    root: "/Users/test/Meetings",
    rootExists: true,
    meetings: [],
  });
  await useAppStore.getState().loadMeetings();
  render(
    <MemoryRouter>
      <Meetings />
    </MemoryRouter>,
  );
}

describe("Meetings list", () => {
  test("every row button is left-aligned so names and dates line up", async () => {
    ipc.listMeetings.mockResolvedValue({
      root: "/Users/test/Meetings",
      rootExists: true,
      meetings: [
        meetingSummary({ id: "a", title: "Meeting" }),
        meetingSummary({ id: "b", title: "A much longer meeting title" }),
      ],
    });
    await useAppStore.getState().loadMeetings();
    render(
      <MemoryRouter>
        <Meetings />
      </MemoryRouter>,
    );

    for (const title of ["Meeting", "A much longer meeting title"]) {
      const row = (await screen.findByText(title)).closest("button");
      expect(row?.className).toContain("text-start");
    }
  });

  test("the empty list points at the shortcut while it is meet-ai's (TUR-169)", async () => {
    shortcut.available = true;
    await renderEmpty();
    expect(await screen.findByText(/^Press ⌘⇧R from anywhere/)).toBeInTheDocument();
  });

  test("a shortcut another app owns is not advertised (TUR-169)", async () => {
    shortcut.available = false;
    await renderEmpty();
    expect(
      await screen.findByText(/⌘⇧R is unavailable because another app is using it/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^Press ⌘⇧R/)).toBeNull();
  });

  // TUR-170: a click while a ⌘⇧R start is under way must not stop it.
  test("Start recording waits while Rust is starting or stopping", async () => {
    shortcut.available = true;
    const idle = { phase: "idle", meetingId: null, startedAtMs: null, error: null } as const;
    useRecordingStore.setState({ status: { ...idle, phase: "starting" }, busy: false });
    await renderEmpty();
    const button = await screen.findByRole("button", { name: "Start recording" });
    expect(button).toBeDisabled();

    act(() => useRecordingStore.setState({ status: idle }));
    expect(button).toBeEnabled();
  });
});
