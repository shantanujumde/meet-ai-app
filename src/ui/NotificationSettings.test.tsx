import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import {
  ATTENDEES_LABEL,
  AUDIO_LABEL,
  CALL_START_LABEL,
  CALL_END_LABEL,
  LEAD_LABEL,
  leadLabel,
  NotificationSettings,
  PROCESSES_LABEL,
  REMIND_LABEL,
} from "./NotificationSettings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const DEFAULTS = {
  remind: true,
  remindBeforeMinutes: 1,
  processes: true,
  audioActivity: true,
  minAttendees: 2,
  callStart: true,
  callEnd: true,
};

async function show() {
  render(<NotificationSettings />);
  // The settings and the OS permission are read on mount.
  await act(async () => {});
}

function select(name: string): HTMLSelectElement {
  return screen.getByRole("combobox", { name }) as HTMLSelectElement;
}

describe("NotificationSettings", () => {
  test("shows what config.jsonc says, with a one-minute lead time by default", async () => {
    await show();
    expect(screen.getByRole("heading", { name: "Notifications" })).toBeTruthy();
    for (const label of [
      REMIND_LABEL,
      PROCESSES_LABEL,
      AUDIO_LABEL,
      CALL_START_LABEL,
      CALL_END_LABEL,
    ]) {
      expect(screen.getByRole("switch", { name: label })).toHaveAttribute("aria-checked", "true");
    }
    expect(select(LEAD_LABEL).value).toBe("1");
    expect([...select(LEAD_LABEL).options].map((option) => option.value)).toEqual([
      "1",
      "2",
      "5",
      "10",
    ]);
    expect(select(ATTENDEES_LABEL).value).toBe("2");
    expect(select(ATTENDEES_LABEL).options).toHaveLength(10);
  });

  test.each([
    [REMIND_LABEL, { remind: false }],
    [PROCESSES_LABEL, { processes: false }],
    [AUDIO_LABEL, { audioActivity: false }],
    [CALL_START_LABEL, { callStart: false }],
    [CALL_END_LABEL, { callEnd: false }],
  ])("switching %s off writes its key", async (label, change) => {
    await show();
    const toggle = screen.getByRole("switch", { name: label });
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(ipc.setNotificationSettings).toHaveBeenCalledWith({ ...DEFAULTS, ...change });
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  test("the lead time writes remind_before_minutes", async () => {
    await show();
    await act(async () => {
      fireEvent.change(select(LEAD_LABEL), { target: { value: "5" } });
    });
    expect(ipc.setNotificationSettings).toHaveBeenCalledWith({
      ...DEFAULTS,
      remindBeforeMinutes: 5,
    });
    expect(select(LEAD_LABEL).value).toBe("5");
  });

  test("the attendee count writes min_attendees", async () => {
    await show();
    await act(async () => {
      fireEvent.change(select(ATTENDEES_LABEL), { target: { value: "4" } });
    });
    expect(ipc.setNotificationSettings).toHaveBeenCalledWith({ ...DEFAULTS, minAttendees: 4 });
    expect(select(ATTENDEES_LABEL).value).toBe("4");
  });

  test("the lead time waits while reminders are off", async () => {
    ipc.notificationSettings.mockResolvedValueOnce({ ...DEFAULTS, remind: false });
    await show();
    expect(select(LEAD_LABEL)).toBeDisabled();
  });

  test("a lead time set by hand is shown as it is", async () => {
    ipc.notificationSettings.mockResolvedValueOnce({ ...DEFAULTS, remindBeforeMinutes: 0 });
    await show();
    expect(select(LEAD_LABEL).value).toBe("0");
    expect(leadLabel(0)).toBe("At the start");
    expect(leadLabel(1)).toBe("1 minute before");
    expect(leadLabel(10)).toBe("10 minutes before");
  });

  test("a failed save keeps the old state and says why", async () => {
    ipc.setNotificationSettings.mockRejectedValueOnce({
      domain: "app",
      kind: "io",
      message: "The meetings folder is moving.",
    });
    await show();
    const toggle = screen.getByRole("switch", { name: PROCESSES_LABEL });
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText(/The meetings folder is moving/)).toBeTruthy();
  });

  test("says nothing about the OS while notifications are allowed", async () => {
    await show();
    expect(screen.queryByText(/blocking meet-ai's notifications/)).toBeNull();
  });

  test("when the OS blocks notifications, links to its settings", async () => {
    ipc.osNotificationsBlocked.mockResolvedValueOnce(true);
    await show();
    expect(screen.getByText("macOS is blocking meet-ai's notifications")).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }));
    });
    expect(ipc.openNotificationSettings).toHaveBeenCalledTimes(1);
  });

  test("Send a test reminder fires one, and never records", async () => {
    await show();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send a test reminder" }));
    });
    expect(ipc.sendTestReminder).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Sent.")).toBeTruthy();
    expect(ipc.toggleRecording).not.toHaveBeenCalled();
    expect(ipc.recordRemindedMeeting).not.toHaveBeenCalled();
  });

  test("a test reminder held back by a recording says so", async () => {
    ipc.sendTestReminder.mockResolvedValueOnce(false);
    await show();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send a test reminder" }));
    });
    expect(screen.getByText("Not while recording.")).toBeTruthy();
  });
});
