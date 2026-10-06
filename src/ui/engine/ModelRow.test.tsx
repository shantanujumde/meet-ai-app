import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import type { ModelView } from "@/ipc/types";
import { ACCURACY_NOTE } from "./ModelLanguages";
import { ModelRow } from "./ModelRow";

/** The (i) button on a whisper model row (TUR-94). */

function model(overrides: Partial<ModelView> = {}): ModelView {
  return {
    id: "large-v3-q5_0",
    filename: "ggml-large-v3-q5_0.bin",
    bytes: 1_081_140_203,
    installed: true,
    displayName: "Large (multilingual)",
    goodFor: "Most accurate. Understands 100 languages.",
    tags: ["most-accurate", "multilingual", "slower"],
    languages: ["English", "Chinese", "Hindi", "Cantonese"],
    recommended: null,
    ...overrides,
  };
}

function renderRow(view: ModelView, onPick = vi.fn()) {
  render(
    <ModelRow
      group="models"
      model={view}
      state={{}}
      picked={false}
      inUse={false}
      onPick={onPick}
      onDownload={vi.fn()}
      onDelete={vi.fn()}
    />,
  );
  return onPick;
}

describe("ModelRow: supported languages", () => {
  test("(i) shows the count and the languages A to Z, with the accuracy note", async () => {
    const user = userEvent.setup();
    renderRow(model());
    const button = screen.getByRole("button", {
      name: "Supported languages for Large (multilingual)",
    });
    expect(button).toHaveAttribute("aria-expanded", "false");
    await user.click(button);

    expect(button).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByRole("region", {
      name: "Supported languages for Large (multilingual)",
    });
    expect(within(panel).getByText("4 languages")).toBeInTheDocument();
    expect(within(panel).getByText("Hindi")).toBeInTheDocument();
    const items = within(panel)
      .getAllByRole("listitem")
      .map((item) => item.textContent);
    expect(items).toEqual(["Cantonese", "Chinese", "English", "Hindi"]);
    expect(within(panel).getByText(ACCURACY_NOTE)).toBeInTheDocument();
    expect(within(panel).getByRole("link", { name: "Source: OpenAI Whisper" })).toHaveAttribute(
      "href",
      "https://github.com/openai/whisper#available-models-and-languages",
    );
  });

  test("an English-only model says so", async () => {
    const user = userEvent.setup();
    renderRow(model({ displayName: "Small (English only)", languages: ["English"] }));
    await user.click(
      screen.getByRole("button", { name: "Supported languages for Small (English only)" }),
    );
    expect(screen.getByText("English only")).toBeInTheDocument();
  });

  test("Escape closes it and puts focus back on the button", async () => {
    const user = userEvent.setup();
    renderRow(model());
    const button = screen.getByRole("button", { name: /Supported languages/ });
    await user.click(button);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("region")).toBeNull();
    expect(button).toHaveFocus();
  });

  test("a click outside closes it", async () => {
    const user = userEvent.setup();
    renderRow(model());
    await user.click(screen.getByRole("button", { name: /Supported languages/ }));
    await user.click(document.body);
    expect(screen.queryByRole("region")).toBeNull();
  });

  test("Enter opens it, and opening it does not pick the model", async () => {
    const user = userEvent.setup();
    const onPick = renderRow(model());
    const button = screen.getByRole("button", { name: /Supported languages/ });
    button.focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("region")).toBeInTheDocument();
    await user.click(button);
    expect(onPick).not.toHaveBeenCalled();
    expect(screen.getByRole("radio")).not.toBeChecked();
  });
});

describe("ModelRow: delete (TUR-132)", () => {
  function renderDeletable(view: ModelView, picked: boolean) {
    const onDelete = vi.fn();
    render(
      <ModelRow
        group="models"
        model={view}
        state={{}}
        picked={picked}
        inUse={false}
        onPick={vi.fn()}
        onDownload={vi.fn()}
        onDelete={onDelete}
      />,
    );
    return onDelete;
  }

  test("no Delete on the picked model", () => {
    renderDeletable(model(), true);
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  });

  test("no Delete on a model that is not downloaded", () => {
    renderDeletable(model({ installed: false }), false);
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  });

  test("Delete asks first, Keep backs out, and confirming calls onDelete", async () => {
    const user = userEvent.setup();
    const onDelete = renderDeletable(model(), false);

    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByText("Delete 1.1 GB?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Keep" }));
    expect(onDelete).not.toHaveBeenCalled();
    expect(screen.queryByText(/\?$/)).toBeNull();

    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
  });
});
