import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { AudioRetentionRow, RETENTION_PAUSED, retentionSentence } from "./AudioRetentionRow";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

describe("AudioRetentionRow", () => {
  test("says how many days audio is kept", async () => {
    render(<AudioRetentionRow />);
    expect(await screen.findByText(/^Audio is kept for 7 days\./)).toBeTruthy();
  });

  test("says when audio goes right after the transcript", async () => {
    ipc.audioRetentionDays.mockResolvedValue({ state: "running", days: 0 });
    render(<AudioRetentionRow />);
    expect(await screen.findByText(/^Audio is deleted once the transcript is done\./)).toBeTruthy();
  });

  test("says when audio is kept forever", async () => {
    ipc.audioRetentionDays.mockResolvedValue({ state: "running", days: -1 });
    render(<AudioRetentionRow />);
    expect(await screen.findByText(/^Audio is kept forever\./)).toBeTruthy();
  });

  // TUR-85: a bad or unreadable config.jsonc never reads as "7 days".
  test("says cleanup is paused, and why, when config.jsonc cannot be read", async () => {
    ipc.audioRetentionDays.mockResolvedValue({
      state: "paused",
      reason: "config.jsonc: audio: retention_days is -5",
    });
    render(<AudioRetentionRow />);
    const line = await screen.findByText(
      /^Audio cleanup is paused because meet-ai could not read its settings file \(config\.jsonc\)/,
    );
    expect(line.textContent).toBe(
      `${RETENTION_PAUSED} (config.jsonc: audio: retention_days is -5). ` +
        "No audio is deleted until it is fixed.",
    );
    expect(screen.queryByText(/7 days/)).toBeNull();
  });

  test("shows nothing when the command itself fails", async () => {
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
