import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { NOTES_AUTO_LABEL, NOTES_MANUAL_LABEL, NotesWhenSetting } from "./NotesWhenSetting";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

function radio(name: string) {
  return screen.getByRole("radio", { name: new RegExp(name) });
}

describe("NotesWhenSetting", () => {
  test("shows the saved choice, automatic by default", async () => {
    render(<NotesWhenSetting />);
    await waitFor(() => expect(radio(NOTES_AUTO_LABEL)).toBeChecked());
    expect(radio(NOTES_MANUAL_LABEL)).not.toBeChecked();
  });

  test("picking Only when I click saves auto_run off and shows what was saved", async () => {
    render(<NotesWhenSetting />);
    await waitFor(() => expect(radio(NOTES_AUTO_LABEL)).toBeChecked());

    fireEvent.click(radio(NOTES_MANUAL_LABEL));
    await waitFor(() => expect(ipc.saveNotesAutoRun).toHaveBeenCalledWith(false));
    await waitFor(() => expect(radio(NOTES_MANUAL_LABEL)).toBeChecked());

    fireEvent.click(radio(NOTES_AUTO_LABEL));
    await waitFor(() => expect(ipc.saveNotesAutoRun).toHaveBeenCalledWith(true));
    await waitFor(() => expect(radio(NOTES_AUTO_LABEL)).toBeChecked());
  });

  test("a saved Manual loads as Manual", async () => {
    ipc.notesAutoRun.mockResolvedValue(false);
    render(<NotesWhenSetting />);
    await waitFor(() => expect(radio(NOTES_MANUAL_LABEL)).toBeChecked());
  });

  test("a failed save keeps the old choice and says why", async () => {
    ipc.saveNotesAutoRun.mockRejectedValueOnce({
      domain: "app",
      kind: "invalid-config",
      message: "Bad config.",
    });
    render(<NotesWhenSetting />);
    await waitFor(() => expect(radio(NOTES_AUTO_LABEL)).toBeChecked());
    fireEvent.click(radio(NOTES_MANUAL_LABEL));
    expect(await screen.findByText("Bad config.")).toBeTruthy();
    expect(radio(NOTES_AUTO_LABEL)).toBeChecked();
  });
});
