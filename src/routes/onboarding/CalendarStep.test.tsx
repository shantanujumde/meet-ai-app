import { act, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { describe, expect, test, vi } from "vitest";
import { ONBOARDING, ONBOARDING_STEPS, onboardingStepPath, onboardingSteps } from "@/lib/routes";
import { ipc } from "@/test/ipcMock";
import { SIGN_IN_TO_SEE_TODAY } from "@/ui/calendar/copy";
import { Onboarding } from "../Onboarding";
import { CalendarStep } from "./CalendarStep";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const OFF_MAC = {
  calendarAppAvailable: false,
  calendarApp: false,
  configured: ["google" as const, "microsoft" as const],
  connected: [],
};

function Location() {
  return <p>at {useLocation().pathname}</p>;
}

async function wizardAt(step: "folder" | "calendar") {
  render(
    <MemoryRouter initialEntries={[onboardingStepPath(step)]}>
      <Routes>
        <Route path={`${ONBOARDING}/:step`} element={<Onboarding />} />
      </Routes>
      <Location />
    </MemoryRouter>,
  );
  await act(async () => {});
}

describe("onboarding's calendar step (TUR-49)", () => {
  test("is only in the wizard off macOS", () => {
    expect(onboardingSteps(true)).not.toContain("calendar");
    expect(onboardingSteps(false)).toEqual(ONBOARDING_STEPS);
    expect(onboardingSteps(false).indexOf("calendar")).toBe(
      onboardingSteps(false).indexOf("agent") - 1,
    );
  });

  test("on a Mac, the folder step goes straight to the agent", async () => {
    await wizardAt("folder");
    expect(screen.getByText(/Step \d+ of 5/)).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    });
    expect(screen.getByText(`at ${onboardingStepPath("agent")}`)).toBeTruthy();
  });

  test("off macOS, the folder step leads to the calendar step", async () => {
    ipc.calendarSources.mockResolvedValue(OFF_MAC);
    await wizardAt("folder");
    expect(screen.getByText(/Step \d+ of 6/)).toBeTruthy();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    });
    expect(screen.getByText(`at ${onboardingStepPath("calendar")}`)).toBeTruthy();
    expect(screen.getByText(new RegExp(SIGN_IN_TO_SEE_TODAY))).toBeTruthy();
  });

  test("Skip moves on without signing in", async () => {
    ipc.calendarSources.mockResolvedValue(OFF_MAC);
    await wizardAt("calendar");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    });
    expect(ipc.calendarConnect).not.toHaveBeenCalled();
    expect(screen.getByText(`at ${onboardingStepPath("agent")}`)).toBeTruthy();
  });

  test("signing in shows the account, then Continue", async () => {
    const onNext = vi.fn();
    render(<CalendarStep onNext={onNext} />);
    expect(screen.getByRole("button", { name: "Sign in with Google" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Sign in with Microsoft" })).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
    });
    expect(ipc.calendarConnect).toHaveBeenCalledWith("google");
    expect(screen.getByRole("status")).toHaveTextContent("Signed in to Google as ada@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(onNext).toHaveBeenCalled();
  });
});
