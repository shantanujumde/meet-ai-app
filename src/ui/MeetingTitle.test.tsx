/** The in-place meeting title (TUR-103): save, cancel, blank and failure paths. */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { MeetingTitle } from "./MeetingTitle";

const OPEN = { name: "Rename meeting: Standup" };

describe("MeetingTitle", () => {
  it("shows the title as a button named after it", () => {
    render(<MeetingTitle title="Standup" onRename={vi.fn()} />);
    expect(screen.getByRole("button", OPEN)).toBeInTheDocument();
  });

  it("saves the trimmed new title on Enter", async () => {
    const onRename = vi.fn(async (_title: string) => {});
    render(<MeetingTitle title="Standup" onRename={onRename} />);
    await userEvent.click(screen.getByRole("button", OPEN));
    const field = screen.getByRole("textbox", { name: "Meeting title" });
    await userEvent.clear(field);
    await userEvent.type(field, "  Planning  {Enter}");
    await waitFor(() => expect(onRename).toHaveBeenCalledTimes(1));
    expect(onRename).toHaveBeenCalledWith("Planning");
  });

  it("puts the old title back on Escape without saving", async () => {
    const onRename = vi.fn(async (_title: string) => {});
    render(<MeetingTitle title="Standup" onRename={onRename} />);
    await userEvent.click(screen.getByRole("button", OPEN));
    await userEvent.type(screen.getByRole("textbox", { name: "Meeting title" }), "x{Escape}");
    expect(screen.getByRole("button", OPEN)).toBeInTheDocument();
    expect(onRename).not.toHaveBeenCalled();
  });

  it("saves nothing for a blank or unchanged title", async () => {
    const onRename = vi.fn(async (_title: string) => {});
    render(<MeetingTitle title="Standup" onRename={onRename} />);
    await userEvent.click(screen.getByRole("button", OPEN));
    await userEvent.clear(screen.getByRole("textbox", { name: "Meeting title" }));
    await userEvent.keyboard("   {Enter}");
    expect(onRename).not.toHaveBeenCalled();
    expect(screen.getByRole("button", OPEN)).toBeInTheDocument();
  });

  it("saves once when clicking away, and shows the reason a save failed", async () => {
    const onRename = vi.fn(async (_title: string) => {
      throw { domain: "app", kind: "unexpected", message: "Disk full" };
    });
    render(<MeetingTitle title="Standup" onRename={onRename} />);
    await userEvent.click(screen.getByRole("button", OPEN));
    await userEvent.type(screen.getByRole("textbox", { name: "Meeting title" }), "2");
    await userEvent.tab();
    await waitFor(() => expect(screen.getByText(/Disk full/)).toBeInTheDocument());
    expect(onRename).toHaveBeenCalledTimes(1);
    expect(onRename).toHaveBeenCalledWith("Standup2");
  });
});
