import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { expect, test, vi } from "vitest";
import { ipc } from "@/test/ipcMock";
import { Onboarding } from "../Onboarding";

/**
 * The agent step is setup's last (docs/problem.md item 63): the folder step
 * now continues to it, and its Done is what finishes onboarding.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), ask: vi.fn() }));

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/onboarding/:step" element={<Onboarding />} />
        <Route path="/meetings" element={<p>Meetings list</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

test("the folder step continues to the agent step, whose Done finishes setup", async () => {
  renderAt("/onboarding/folder");

  expect(screen.getByText("Step 4 of 5")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Done" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));

  expect(await screen.findByRole("heading", { name: "Who writes your notes" })).toBeTruthy();
  expect(screen.getByText("Step 5 of 5")).toBeInTheDocument();
  expect(await screen.findByRole("radio", { name: /Claude Code/ })).toBeInTheDocument();
  await waitFor(() => expect(ipc.detectAgents).toHaveBeenCalled());

  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(ipc.completeOnboarding).toHaveBeenCalled());
  expect(await screen.findByText("Meetings list")).toBeInTheDocument();
});

test("Back from the agent step goes to the folder step", async () => {
  renderAt("/onboarding/agent");
  await screen.findByRole("heading", { name: "Who writes your notes" });

  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(await screen.findByRole("heading", { name: "Where your meetings live" })).toBeTruthy();
});
