import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { AudioRetentionRow, retentionSentence } from "./AudioRetentionRow";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

describe("AudioRetentionRow", () => {
  test("says how many days audio is kept", async () => {
    render(<AudioRetentionRow />);
    expect(await screen.findByText(/^Audio is kept for 7 days\./)).toBeTruthy();
  });

  test("says when audio goes right after the transcript", async () => {
    ipc.audioRetentionDays.mockResolvedValue(0);
    render(<AudioRetentionRow />);
    expect(await screen.findByText(/^Audio is deleted once the transcript is done\./)).toBeTruthy();
  });

  test("says when audio is kept forever", async () => {
    ipc.audioRetentionDays.mockResolvedValue(-1);
    render(<AudioRetentionRow />);
    expect(await screen.findByText(/^Audio is kept forever\./)).toBeTruthy();
  });

  test("shows nothing when the setting cannot be read", async () => {
    ipc.audioRetentionDays.mockRejectedValue(new Error("no"));
    const { container } = render(<AudioRetentionRow />);
    await Promise.resolve();
    expect(container.textContent).toBe("");
  });

  test("one day is singular", () => {
    expect(retentionSentence(1)).toBe("Audio is kept for 1 day");
    expect(retentionSentence(30)).toBe("Audio is kept for 30 days");
  });
});
