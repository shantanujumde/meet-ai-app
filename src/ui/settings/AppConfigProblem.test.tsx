/** TUR-170: the Menu bar section shows the `app` section's invalid-config note. */

import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { DOCK_SETTING_LABEL, DockSetting } from "../DockSetting";
import { MENU_BAR_COUNTDOWN_LABEL, MenuBarCountdownSetting } from "../MenuBarCountdownSetting";
import { AppConfigProblem } from "./AppConfigProblem";
import { CONFIG_PROBLEM_LEAD } from "./useConfigProblem";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const PROBLEM = 'app.show_in_dock_when_closed "yes" is not true or false';

async function show() {
  render(
    <>
      <DockSetting />
      <MenuBarCountdownSetting />
      <AppConfigProblem />
    </>,
  );
  await act(async () => {});
}

test("a bad value in the app section shows the note, asked for that section", async () => {
  ipc.configProblem.mockResolvedValue(PROBLEM);
  await show();
  expect(ipc.configProblem).toHaveBeenCalledWith("app");
  expect(screen.getByRole("alert")).toHaveTextContent(`${CONFIG_PROBLEM_LEAD} ${PROBLEM}`);
});

test("a save from either switch asks again, so a fixed value clears the note", async () => {
  ipc.configProblem.mockResolvedValue(PROBLEM);
  await show();
  expect(screen.getByRole("alert")).toBeInTheDocument();

  ipc.configProblem.mockResolvedValue(null);
  await act(async () => {
    fireEvent.click(screen.getByRole("switch", { name: DOCK_SETTING_LABEL }));
  });
  expect(screen.queryByRole("alert")).toBeNull();

  ipc.configProblem.mockClear();
  await act(async () => {
    fireEvent.click(screen.getByRole("switch", { name: MENU_BAR_COUNTDOWN_LABEL }));
  });
  expect(ipc.configProblem).toHaveBeenCalledWith("app");
});

test("all valid: no note", async () => {
  ipc.configProblem.mockResolvedValue(null);
  await show();
  expect(screen.queryByRole("alert")).toBeNull();
});
