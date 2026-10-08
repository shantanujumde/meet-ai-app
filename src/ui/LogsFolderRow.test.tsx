import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { Settings } from "@/routes/Settings";
import { ipc } from "@/test/ipcMock";
import { LogsFolderRow } from "./LogsFolderRow";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const { openLogsFolder } = ipc;

describe("LogsFolderRow", () => {
  test("Settings has an Open logs folder button that opens the folder", async () => {
    render(
      <MemoryRouter>
        <Settings />
      </MemoryRouter>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Open logs folder" }));

    expect(openLogsFolder).toHaveBeenCalledTimes(1);
  });

  test("says why when the folder will not open", async () => {
    openLogsFolder.mockRejectedValue(new Error("No file manager."));
    render(<LogsFolderRow />);

    fireEvent.click(screen.getByRole("button", { name: "Open logs folder" }));

    expect(await screen.findByText(/No file manager\./)).toBeTruthy();
  });
});
