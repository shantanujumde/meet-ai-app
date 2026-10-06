/**
 * The meeting row menu (TUR-116), as the sidebar and the Meetings list show
 * it: the ⋯ button and a right-click open the same items, Delete asks first,
 * and the meeting being recorded cannot be deleted.
 */

import { ask } from "@tauri-apps/plugin-dialog";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import type { MeetingSummary, RecordingStatus } from "@/ipc/types";
import { Meetings } from "@/routes/Meetings";
import { useAppStore } from "@/state/app";
import { meetingDetail, meetingSummary, ticketSummary, transcriptLine } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { Sidebar } from "./Sidebar";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(), message: vi.fn() }));
const writeText = vi.hoisted(() => vi.fn(async (_text: string) => {}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText }));

const confirm = vi.mocked(ask);

// jsdom has no layout: Radix's popper measures with these.
beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  Element.prototype.scrollIntoView ??= () => {};
  Element.prototype.hasPointerCapture ??= () => false;
  Element.prototype.releasePointerCapture ??= () => {};
});

beforeEach(() => {
  confirm.mockReset();
  writeText.mockClear();
});

const ROOT = "/Users/test/Meetings";
const STANDUP = meetingSummary({ id: "2026-09-30-0900-standup", title: "Platform standup" });
const REVIEW = meetingSummary({ id: "2026-09-30-1400-review", title: "Design review" });

const IDLE: RecordingStatus = { phase: "idle", meetingId: null, startedAtMs: null, error: null };

function recordingInto(id: string): RecordingStatus {
  return { phase: "recording", meetingId: id, startedAtMs: 0, error: null };
}

function Where() {
  return <span data-testid="where">{useLocation().pathname}</span>;
}

/** The sidebar fed from the store, as the shell feeds it. */
function LiveSidebar({
  recording,
  selectedId,
}: {
  recording: RecordingStatus;
  selectedId?: string;
}) {
  const list = useAppStore((state) => state.meetings);
  return <Sidebar list={list} loading={false} recording={recording} selectedId={selectedId} />;
}

async function showSidebar(
  meetings: MeetingSummary[],
  { at = "/meetings", recording = IDLE, selectedId = undefined as string | undefined } = {},
) {
  ipc.listMeetings.mockResolvedValue({ root: ROOT, rootExists: true, meetings });
  await useAppStore.getState().loadMeetings();
  render(
    <MemoryRouter initialEntries={[at]}>
      <LiveSidebar recording={recording} selectedId={selectedId} />
      <Routes>
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
  return screen.getByRole("navigation", { name: "Sidebar" });
}

function moreButton(title: string) {
  return screen.getByRole("button", { name: `More actions for ${title}` });
}

/** The labels of the open menu's items, in order. */
function itemLabels(): string[] {
  return within(screen.getByRole("menu"))
    .getAllByRole("menuitem")
    .map((item) => item.textContent ?? "");
}

async function openWithButton(title: string) {
  await userEvent.click(moreButton(title));
  await screen.findByRole("menu");
}

async function openWithRightClick(row: HTMLElement) {
  await act(async () => {
    fireEvent.contextMenu(row, { clientX: 10, clientY: 10 });
  });
  await screen.findByRole("menu");
}

async function pick(label: string) {
  await userEvent.click(screen.getByRole("menuitem", { name: label }));
}

describe("the meeting row menu (TUR-116)", () => {
  test("the ⋯ button and a right-click open the same items", async () => {
    const nav = await showSidebar([STANDUP]);

    await openWithButton("Platform standup");
    const fromButton = itemLabels();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());

    const row = within(nav).getByText("Platform standup").closest("button") as HTMLElement;
    await openWithRightClick(row);
    expect(itemLabels()).toEqual(fromButton);
    expect(fromButton).toEqual([
      "Open",
      "Show in Finder",
      "Copy folder path",
      "Write notes now",
      "Turn notes off for this meeting",
      "Copy transcript",
      "Rename…",
      "Delete…",
    ]);
  });

  test("Shift+F10 on a focused row opens the menu", async () => {
    const nav = await showSidebar([STANDUP]);
    const row = within(nav).getByText("Platform standup").closest("button") as HTMLElement;
    row.focus();
    await userEvent.keyboard("{Shift>}{F10}{/Shift}");
    await screen.findByRole("menu");
    expect(itemLabels()).toContain("Delete…");
  });

  test("the ⋯ button is named for its meeting", async () => {
    await showSidebar([STANDUP, REVIEW]);
    expect(moreButton("Platform standup")).toBeInTheDocument();
    expect(moreButton("Design review")).toBeInTheDocument();
  });

  test("Delete, cancelled, deletes nothing", async () => {
    await showSidebar([STANDUP]);
    confirm.mockResolvedValue(false);

    await openWithButton("Platform standup");
    await pick("Delete…");

    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    expect(confirm).toHaveBeenCalledWith(
      "Its transcript, notes and audio will be moved to the Trash.",
      expect.objectContaining({
        title: "Delete “Platform standup”?",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Cancel",
      }),
    );
    expect(ipc.deleteMeeting).not.toHaveBeenCalled();
    expect(screen.getByText("Platform standup")).toBeInTheDocument();
  });

  test("Delete, confirmed, moves it to the Trash and drops it from the list", async () => {
    const nav = await showSidebar([STANDUP, REVIEW], {
      at: `/meetings/${STANDUP.id}`,
      selectedId: STANDUP.id,
    });
    confirm.mockResolvedValue(true);
    // After the delete, Rust lists only the other meeting.
    ipc.deleteMeeting.mockImplementation(async () => {
      ipc.listMeetings.mockResolvedValue({ root: ROOT, rootExists: true, meetings: [REVIEW] });
    });

    await openWithButton("Platform standup");
    await pick("Delete…");

    await waitFor(() => expect(ipc.deleteMeeting).toHaveBeenCalledWith(STANDUP.id));
    await waitFor(() =>
      expect(within(nav).queryByText("Platform standup")).not.toBeInTheDocument(),
    );
    expect(within(nav).getByText("Design review")).toBeInTheDocument();
    // It was open, so the window goes to the Meetings list.
    expect(screen.getByTestId("where")).toHaveTextContent(/^\/meetings$/);
  });

  test("Delete says shared tickets stay when the meeting has any", async () => {
    await showSidebar([STANDUP]);
    ipc.listTickets.mockResolvedValue([ticketSummary({ id: "TUR-9", meeting: STANDUP.id })]);
    confirm.mockResolvedValue(false);

    await openWithButton("Platform standup");
    await pick("Delete…");

    await waitFor(() =>
      expect(confirm).toHaveBeenCalledWith(
        "Its transcript, notes and audio will be moved to the Trash. Tickets made from it stay in Tickets.",
        expect.anything(),
      ),
    );
  });

  test("the meeting being recorded cannot be deleted from its menu", async () => {
    const nav = await showSidebar([STANDUP], { recording: recordingInto(STANDUP.id) });

    await openWithButton("Platform standup");
    expect(itemLabels()).toEqual(["Open", "Show in Finder"]);
    await userEvent.keyboard("{Escape}");

    const row = within(nav).getByText("Platform standup").closest("button") as HTMLElement;
    await openWithRightClick(row);
    expect(itemLabels()).toEqual(["Open", "Show in Finder"]);
    expect(screen.queryByRole("menuitem", { name: "Delete…" })).not.toBeInTheDocument();
  });

  test("Copy transcript puts the transcript.md lines on the clipboard", async () => {
    await showSidebar([STANDUP]);
    ipc.readMeeting.mockResolvedValue(
      meetingDetail({
        summary: STANDUP,
        lines: [transcriptLine({ time: "00:00:04", speaker: "Others", text: "Morning." })],
      }),
    );

    await openWithButton("Platform standup");
    await pick("Copy transcript");

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("[00:00:04] Others: Morning."));
  });

  test("no agent set up: the menu offers Copy prompt once it has asked", async () => {
    await showSidebar([STANDUP]);
    ipc.copyPromptFallback.mockResolvedValue(true);

    await openWithButton("Platform standup");
    await screen.findByRole("menuitem", { name: "Copy prompt" });
    expect(screen.queryByRole("menuitem", { name: "Write notes now" })).not.toBeInTheDocument();
  });

  test("a notes run going: Stop writing notes cancels it", async () => {
    await showSidebar([STANDUP]);
    ipc.notesRunStatus.mockResolvedValue({ meetingId: STANDUP.id, state: { state: "running" } });

    await openWithButton("Platform standup");
    await screen.findByRole("menuitem", { name: "Stop writing notes" });
    await pick("Stop writing notes");

    await waitFor(() => expect(ipc.cancelNotesRun).toHaveBeenCalledWith(STANDUP.id));
  });

  test("Rename… edits the title in place and saves it", async () => {
    const nav = await showSidebar([STANDUP]);

    await openWithButton("Platform standup");
    await pick("Rename…");

    const field = await within(nav).findByRole("textbox", { name: "Meeting title" });
    await waitFor(() => expect(field).toHaveFocus());
    await userEvent.clear(field);
    await userEvent.type(field, "Weekly sync{Enter}");

    expect(ipc.renameMeeting).toHaveBeenCalledWith(STANDUP.id, "Weekly sync");
    expect(within(nav).queryByRole("textbox", { name: "Meeting title" })).not.toBeInTheDocument();
  });
});

describe("the Meetings list rows (TUR-116)", () => {
  test("each row has the same menu, from ⋯ and from a right-click", async () => {
    ipc.listMeetings.mockResolvedValue({ root: ROOT, rootExists: true, meetings: [STANDUP] });
    await useAppStore.getState().loadMeetings();
    render(
      <MemoryRouter>
        <Meetings />
      </MemoryRouter>,
    );

    await openWithButton("Platform standup");
    const fromButton = itemLabels();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());

    const row = (await screen.findByText("Platform standup")).closest("button") as HTMLElement;
    await openWithRightClick(row);
    expect(itemLabels()).toEqual(fromButton);
    expect(fromButton).toContain("Delete…");
  });
});
