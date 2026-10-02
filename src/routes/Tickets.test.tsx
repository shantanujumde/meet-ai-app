import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import type { TicketSummary } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { ipc } from "@/test/ipcMock";
import { Tickets } from "./Tickets";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

const { listTickets, createTicket, startWorkPrompt } = ipc;

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
    ...over,
  };
}

function renderTickets() {
  return render(
    <MemoryRouter>
      <Tickets />
    </MemoryRouter>,
  );
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
