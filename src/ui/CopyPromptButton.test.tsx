import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { copyText } from "@/lib/clipboard";
import { COPIED_RESET_MS } from "@/lib/constants";
import { CopyPromptButton } from "./CopyPromptButton";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

const copy = vi.mocked(copyText);

describe("CopyPromptButton", () => {
  afterEach(() => {
    copy.mockReset();
    vi.useRealTimers();
  });

  test("copies what Rust rendered, says Copied, then reads normally again", async () => {
    const render$ = vi.fn(async () => "Do the thing.");
    render(<CopyPromptButton label="Start Work" render={render$} />);

    vi.useFakeTimers({ shouldAdvanceTime: true });
    fireEvent.click(screen.getByRole("button", { name: "Start Work" }));

    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
    expect(render$).toHaveBeenCalledTimes(1);
    expect(copy).toHaveBeenCalledWith("Do the thing.");
    expect(screen.getByRole("status").textContent).toBe("Prompt copied to the clipboard");

    act(() => {
      vi.advanceTimersByTime(COPIED_RESET_MS);
    });
    expect(screen.getByRole("button", { name: "Start Work" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toBe("");
  });

  test("is disabled while the prompt is being made, keeping its label", async () => {
    let finish: (prompt: string) => void = () => {};
    const pending = new Promise<string>((resolve) => {
      finish = resolve;
    });
    render(<CopyPromptButton label="Copy prompt" render={() => pending} />);

    const button = screen.getByRole("button", { name: "Copy prompt" }) as HTMLButtonElement;
    fireEvent.click(button);
    await waitFor(() => expect(button.disabled).toBe(true));
    expect(button.textContent).toBe("Copy prompt");

    await act(async () => finish("Wrap up."));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
  });

  test("a prompt Rust could not make is shown as an error, and nothing is copied", async () => {
    render(
      <CopyPromptButton
        label="Start Work"
        render={() =>
          Promise.reject({
            domain: "app",
            kind: "prompt-template",
            message: "start-work.md has an unknown placeholder {{ticket.nope}}.",
          })
        }
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Start Work" }));

    expect(
      await screen.findByText("start-work.md has an unknown placeholder {{ticket.nope}}."),
    ).toBeTruthy();
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(copy).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Start Work" })).toBeTruthy();
  });

  test("a refused clipboard shows the prompt, selected, to copy by hand", async () => {
    copy.mockRejectedValueOnce(new Error("not allowed"));
    render(<CopyPromptButton label="Start Work" render={async () => "Do the thing."} />);

    fireEvent.click(screen.getByRole("button", { name: "Start Work" }));

    const field = (await screen.findByLabelText("Prompt to copy")) as HTMLTextAreaElement;
    expect(field.value).toBe("Do the thing.");
    expect(field.readOnly).toBe(true);
    expect(document.activeElement).toBe(field);
    expect(field.selectionStart).toBe(0);
    expect(field.selectionEnd).toBe("Do the thing.".length);
    expect(
      screen.getByText(
        "Couldn't copy to the clipboard. Select the prompt below and copy it yourself.",
      ),
    ).toBeTruthy();
    // Not an error screen, and the button never claims it copied.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("button", { name: "Copied" })).toBeNull();
  });

  test("shows its hint beside the button", () => {
    render(<CopyPromptButton label="Copy prompt" hint="Paste it into Codex." render={vi.fn()} />);
    expect(screen.getByText("Paste it into Codex.")).toBeTruthy();
  });
});
