import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { CalendarAccount, CalendarSources } from "@/ipc/client";
import { ipc } from "@/test/ipcMock";
import { CALENDAR_APP_LABEL, CalendarSettings, SIGN_IN_EXPIRED } from "./CalendarSettings";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const MAC: CalendarSources = {
  calendarAppAvailable: true,
  calendarApp: true,
  configured: ["google", "microsoft"],
  connected: [],
};

const OFF_MAC: CalendarSources = { ...MAC, calendarAppAvailable: false, calendarApp: false };

function account(over: Partial<CalendarAccount> = {}): CalendarAccount {
  return { provider: "google", account: null, state: "signed-out", remembered: true, ...over };
}

async function show(sources: CalendarSources, accounts?: CalendarAccount[]) {
  ipc.calendarSources.mockResolvedValue(sources);
  if (accounts) ipc.calendarAccounts.mockResolvedValue(accounts);
  render(<CalendarSettings />);
  await act(async () => {});
}

const row = (name: string) => screen.getByRole("group", { name });

describe("CalendarSettings", () => {
  test("on a Mac: the Calendar app switch, on by default, and both sign-ins", async () => {
    await show(MAC);
    expect(screen.getByRole("switch", { name: CALENDAR_APP_LABEL })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(within(row("Google")).getByRole("button", { name: "Sign in with Google" })).toBeTruthy();
    expect(
      within(row("Microsoft")).getByRole("button", { name: "Sign in with Microsoft" }),
    ).toBeTruthy();
  });

  test("off macOS: only the two sign-ins, no Calendar app", async () => {
    await show(OFF_MAC);
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByText(CALENDAR_APP_LABEL)).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeTruthy();
  });

  test("turning the Calendar app off saves it", async () => {
    await show(MAC);
    const toggle = screen.getByRole("switch", { name: CALENDAR_APP_LABEL });
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(ipc.setCalendarApp).toHaveBeenCalledWith(false);
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  test("a signed-in account is shown with Disconnect", async () => {
    await show({ ...OFF_MAC, connected: ["google"] }, [
      account({ account: "ada@example.com", state: "signed-in" }),
      account({ provider: "microsoft" }),
    ]);
    const google = row("Google");
    expect(google).toHaveTextContent("ada@example.com");

    await act(async () => {
      fireEvent.click(within(google).getByRole("button", { name: "Disconnect" }));
    });
    expect(ipc.calendarDisconnect).toHaveBeenCalledWith("google");
    expect(within(row("Google")).getByRole("button", { name: "Sign in with Google" })).toBeTruthy();
  });

  test("signing in shows the account it signed in to", async () => {
    await show(OFF_MAC);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign in with Microsoft" }));
    });
    expect(ipc.calendarConnect).toHaveBeenCalledWith("microsoft");
    expect(row("Microsoft")).toHaveTextContent("ada@example.com");
    expect(within(row("Microsoft")).getByRole("button", { name: "Disconnect" })).toBeTruthy();
  });

  test("an expired sign-in says so and offers Sign in again", async () => {
    await show({ ...MAC, connected: ["microsoft"] }, [
      account(),
      account({ provider: "microsoft", account: "ada@contoso.com", state: "expired" }),
    ]);
    const microsoft = row("Microsoft");
    expect(microsoft).toHaveTextContent(SIGN_IN_EXPIRED);
    await act(async () => {
      fireEvent.click(within(microsoft).getByRole("button", { name: "Sign in again" }));
    });
    expect(ipc.calendarConnect).toHaveBeenCalledWith("microsoft");
  });

  test("no client id: the key to add, instead of a sign-in button", async () => {
    await show({ ...OFF_MAC, configured: ["microsoft"] });
    const google = row("Google");
    expect(google).toHaveTextContent(
      "Add calendar.google.client_id to config.jsonc (see SETUP.md)",
    );
    expect(within(google).queryByRole("button")).toBeNull();
    expect(within(row("Microsoft")).getByRole("button")).toHaveTextContent(
      "Sign in with Microsoft",
    );
  });

  test("a sign-in that fails shows why, on its own row", async () => {
    ipc.calendarConnect.mockRejectedValue({
      domain: "app",
      kind: "calendar-sign-in-cancelled",
      message: "The Google sign-in was cancelled or timed out.",
    });
    await show(MAC);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent("cancelled or timed out");
  });
});
