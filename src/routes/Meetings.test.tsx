import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { useAppStore } from "@/state/app";
import { meetingSummary } from "@/test/fixtures";
import { ipc } from "@/test/ipcMock";
import { Meetings } from "./Meetings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

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
});
