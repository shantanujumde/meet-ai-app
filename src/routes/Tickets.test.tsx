import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { TICKET_SYNC_EVENT, type TicketSyncStatus } from "@/ipc/client";
import type { TicketSummary } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { emit, ipc, listening } from "@/test/ipcMock";
import { Tickets } from "./Tickets";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

const { listTickets, createTicket, startWorkPrompt, ticketSyncStates, retryTicketSync } = ipc;

function ticket(over: Partial<TicketSummary> = {}): TicketSummary {
  return {
    id: "TUR-7",
    title: "Write the docs",
    status: "open",
    meeting: null,
    body: "Cover the setup.",
    hasProblems: false,
    syncedTo: null,
    externalId: null,
    externalUrl: null,
    suggested: false,
    owner: null,
    due: null,
    meetingTitle: null,
    ...over,
  };
}

function Where() {
  const location = useLocation();
  return <p data-testid="where">{`${location.pathname}${location.search}`}</p>;
}

function renderTickets() {
  return render(
    <MemoryRouter initialEntries={["/tickets"]}>
      <Routes>
        <Route path="/tickets" element={<Tickets />} />
        <Route path="*" element={<Where />} />
      </Routes>
    </MemoryRouter>,
  );
}

function tracked(tickets: TicketSyncStatus[] = []) {
  ticketSyncStates.mockResolvedValue({
    trackerSetUp: true,
    tracker: "linear",
    tickets,
  });
}

describe("Tickets", () => {
  test("lists the tickets with id, title, status and body", async () => {
    listTickets.mockResolvedValue([
      ticket(),
      ticket({ id: "TUR-8", title: "Ship", status: "done" }),
    ]);
    renderTickets();

    expect(await screen.findByText("Write the docs")).toBeTruthy();
    expect(screen.getByText("TUR-7")).toBeTruthy();
    expect(screen.getByText("Open")).toBeTruthy();
    expect(screen.getByText("Done")).toBeTruthy();
    expect(screen.getAllByText("Cover the setup.")).toHaveLength(2);
  });

  test("shows an empty state when there are none", async () => {
    listTickets.mockResolvedValue([]);
    renderTickets();
    expect(await screen.findByRole("heading", { name: "No tickets yet" })).toBeTruthy();
  });

  test("creating a ticket sends title and body and shows it first", async () => {
    listTickets.mockResolvedValue([ticket()]);
    createTicket.mockResolvedValue(ticket({ id: "TUR-9", title: "Fresh one", body: "Details" }));
    renderTickets();

    fireEvent.click(await screen.findByRole("button", { name: "New ticket" }));
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Fresh one" } });
    fireEvent.change(screen.getByLabelText("Details"), { target: { value: "Details" } });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    await waitFor(() => expect(createTicket).toHaveBeenCalledWith("Fresh one", "Details"));
    expect(await screen.findByText("TUR-9")).toBeTruthy();
    expect(screen.queryByLabelText("Title")).toBeNull();
    const titles = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(titles).toEqual(["Fresh one", "Write the docs"]);
  });

  test("an empty title blocks submit", async () => {
    listTickets.mockResolvedValue([]);
    renderTickets();

    fireEvent.click(await screen.findByRole("button", { name: "New ticket" }));
    const create = screen.getByRole("button", { name: "Create" }) as HTMLButtonElement;
    expect(create.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "   " } });
    expect(create.disabled).toBe(true);
    expect(createTicket).not.toHaveBeenCalled();
  });

  test("a failed create keeps the form and shows the error", async () => {
    listTickets.mockResolvedValue([]);
    createTicket.mockRejectedValue({ domain: "app", kind: "x", message: "Disk is full." });
    renderTickets();

    fireEvent.click(await screen.findByRole("button", { name: "New ticket" }));
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Oops" } });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    expect(await screen.findByText("Disk is full.")).toBeTruthy();
    expect(screen.getByLabelText("Title")).toBeTruthy();
  });
});

describe("Start Work", () => {
  test("each ticket's button copies the prompt Rust makes for that ticket", async () => {
    vi.mocked(copyText).mockReset();
    listTickets.mockResolvedValue([
      ticket({ id: "TUR-7", meeting: "2026-09-30-1015-meeting" }),
      ticket({ id: "TUR-8", title: "Hand-made", meeting: null }),
    ]);
    startWorkPrompt.mockImplementation(async (id) => `Start work on ${id}.`);
    renderTickets();

    await screen.findByText("Hand-made");
    const buttons = screen.getAllByRole("button", { name: "Start Work" });
    expect(buttons).toHaveLength(2);

    fireEvent.click(buttons[0] as HTMLElement);
    await waitFor(() =>
      expect(startWorkPrompt).toHaveBeenCalledWith("TUR-7", "2026-09-30-1015-meeting"),
    );
    await waitFor(() => expect(copyText).toHaveBeenCalledWith("Start work on TUR-7."));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Start Work" }));
    await waitFor(() => expect(startWorkPrompt).toHaveBeenCalledWith("TUR-8", null));
    await waitFor(() => expect(copyText).toHaveBeenCalledWith("Start work on TUR-8."));
  });
});

describe("Tickets, sending to the tracker (TUR-113)", () => {
  test("with no tracker: the stay-local hint and Set up a tracker opens Settings, Tracker", async () => {
    listTickets.mockResolvedValue([ticket()]);
    renderTickets();

    expect(
      await screen.findByText(
        "Tickets you approved from meetings, and ones you added. They stay on this Mac until you connect a tracker.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("Not sent")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Set up a tracker" }));
    expect((await screen.findByTestId("where")).textContent).toBe("/settings?section=tracker");
  });

  test("with a tracker: the hint names it and each state has its label", async () => {
    tracked([
      { ticketId: "TUR-2", state: "sending", error: null, ticket: null },
      { ticketId: "TUR-3", state: "queued", error: null, ticket: null },
    ]);
    listTickets.mockResolvedValue([
      ticket({ id: "TUR-1", title: "One" }),
      ticket({ id: "TUR-2", title: "Two" }),
      ticket({ id: "TUR-4", title: "Four", syncedTo: "linear", externalId: "ENG-42" }),
    ]);
    renderTickets();

    expect(
      await screen.findByText(
        "Tickets you approved from meetings, and ones you added. Each one is sent to Linear on its own.",
      ),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Set up a tracker" })).toBeNull();
    expect(screen.getByText("Not sent")).toBeTruthy();
    expect(screen.getByText("Sending to Linear…")).toBeTruthy();
    expect(screen.getByText("In Linear: ENG-42")).toBeTruthy();
  });

  test("a sent ticket with a link opens the issue through Rust", async () => {
    listTickets.mockResolvedValue([
      ticket({ syncedTo: "linear", externalId: "ENG-42", externalUrl: "https://linear.app/x" }),
    ]);
    renderTickets();
    fireEvent.click(
      await screen.findByRole("button", { name: "In Linear: ENG-42, open the issue" }),
    );
    await waitFor(() => expect(ipc.openSyncedIssue).toHaveBeenCalledWith("TUR-7", null));
  });

  test("events move a ticket to Sending and then In Linear", async () => {
    tracked();
    listTickets.mockResolvedValue([ticket()]);
    renderTickets();
    await screen.findByText("Not sent");
    await waitFor(() => expect(listening(TICKET_SYNC_EVENT)).toBe(true));

    act(() =>
      emit(TICKET_SYNC_EVENT, { ticketId: "TUR-7", state: "sending", error: null, ticket: null }),
    );
    expect(await screen.findByText("Sending to Linear…")).toBeTruthy();
    act(() =>
      emit(TICKET_SYNC_EVENT, {
        ticketId: "TUR-7",
        state: "sent",
        error: null,
        ticket: ticket({ syncedTo: "linear", externalId: "ENG-7" }),
      }),
    );
    expect(await screen.findByText("In Linear: ENG-7")).toBeTruthy();
  });

  test("an error wraps under the row with Retry and Open Tracker settings", async () => {
    const message =
      "Couldn't send to Linear: your agent couldn't reach Linear. Check that \"claude.ai Linear\" is connected and signed in in Claude Code, then press Retry.";
    tracked([
      {
        ticketId: "TUR-7",
        state: "failed",
        error: { domain: "app", kind: "sync-unreachable", message },
        ticket: null,
      },
    ]);
    listTickets.mockResolvedValue([ticket()]);
    renderTickets();

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe(message);
    expect(alert.className).toContain("wrap-anywhere");
    expect(screen.getByText("Couldn't send to Linear")).toBeTruthy();
    // Under the row: not inside the title's column, but the card's own child.
    const card = screen.getByRole("listitem");
    const title = screen.getByRole("heading", { name: "Write the docs" });
    expect(title.parentElement?.contains(alert)).toBe(false);
    const errorBlock = alert.parentElement as HTMLElement;
    expect(errorBlock.parentElement).toBe(card);
    expect(errorBlock.className).toContain("w-full");

    fireEvent.click(within(card).getByRole("button", { name: "Retry sending TUR-7" }));
    await waitFor(() => expect(retryTicketSync).toHaveBeenCalledWith("TUR-7"));
    fireEvent.click(within(card).getByRole("button", { name: "Open Tracker settings" }));
    expect((await screen.findByTestId("where")).textContent).toBe("/settings?section=tracker");
  });

  test("a signed-out agent offers Retry but not Open Tracker settings", async () => {
    tracked([
      {
        ticketId: "TUR-7",
        state: "failed",
        error: {
          domain: "app",
          kind: "agent-not-signed-in",
          message: "Couldn't send: Claude Code isn't signed in.",
        },
        ticket: null,
      },
    ]);
    listTickets.mockResolvedValue([ticket()]);
    renderTickets();
    expect(await screen.findByText("Couldn't send: Claude Code isn't signed in.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry sending TUR-7" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Open Tracker settings" })).toBeNull();
  });

  test("a meeting ticket links to its meeting; description, owner and due show", async () => {
    listTickets.mockResolvedValue([
      ticket({
        meeting: "2026-09-30-1015-standup",
        meetingTitle: "Standup",
        body: "Add a close button.",
        owner: "Sam",
        due: "Friday",
      }),
      ticket({ id: "TUR-8", title: "Bare", body: "" }),
    ]);
    renderTickets();
    const from = await screen.findByRole("link", { name: "From: Standup" });
    expect(from.getAttribute("href")).toBe("/meetings/2026-09-30-1015-standup");
    expect(screen.getByText("Add a close button.")).toBeTruthy();
    expect(screen.getByText("Owner: Sam")).toBeTruthy();
    expect(screen.getByText("Due: Friday")).toBeTruthy();
    expect(screen.getByText("No description")).toBeTruthy();
  });
});
