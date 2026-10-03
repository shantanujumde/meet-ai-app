import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { AgentCli, AgentModel } from "@/ipc/types";
import { CLAUDE_MODELS, ipc } from "@/test/ipcMock";
import { AgentSetup } from "./AgentSetup";
import { defaultModelText, modelChoices } from "./agents";

/**
 * The model picker (TUR-74): Default (the agent picks) and two models as
 * buttons, every other model the agent offers in a dropdown, and free text
 * for names the list does not have yet.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), ask: vi.fn() }));

const { agentChoice, detectAgents, saveAgentChoice, testAgent } = ipc;

const CODEX_MODELS: AgentModel[] = ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5"].map(
  (name) => ({ name, label: name, note: null }),
);

function claude(overrides: Partial<AgentCli> = {}): AgentCli {
  return {
    id: "claude-code",
    name: "Claude Code",
    provider: "Anthropic",
    state: "ready",
    path: "/usr/local/bin/claude",
    version: "2.1.286",
    signInCommand: "claude auth login",
    models: CLAUDE_MODELS,
    cliDefault: null,
    canTest: true,
    ...overrides,
  };
}

function codex(overrides: Partial<AgentCli> = {}): AgentCli {
  return {
    id: "codex",
    name: "Codex",
    provider: "OpenAI",
    state: "ready",
    path: "/opt/homebrew/bin/codex",
    version: "0.152.1",
    signInCommand: "codex login",
    models: CODEX_MODELS,
    cliDefault: null,
    canTest: true,
    ...overrides,
  };
}

async function renderSetup() {
  render(<AgentSetup />);
  await waitFor(() => expect(detectAgents).toHaveBeenCalled());
  await waitFor(() => expect(screen.queryByText("Checking…")).toBeNull());
}

function field(): HTMLInputElement {
  return screen.getByRole("textbox", { name: /Model/ }) as HTMLInputElement;
}

function dropdown(): HTMLSelectElement {
  return screen.getByRole("combobox", { name: "More models" }) as HTMLSelectElement;
}

/** The suggestion buttons, by their text, in order. */
function chips(): string[] {
  const row = screen.getByText("Suggestions:").parentElement;
  if (!row) throw new Error("no suggestions row");
  return within(row)
    .getAllByRole("button")
    .map((button) => button.textContent ?? "");
}

describe("the model picker", () => {
  test("Claude Code shows Default, Sonnet and Haiku as buttons, and every other model in the dropdown", async () => {
    await renderSetup();

    expect(chips()).toEqual(["Default", "Sonnet", "Haiku"]);
    const options = within(dropdown())
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(options).toEqual([
      "More models…",
      "Opus: most capable, slowest",
      "claude-sonnet-5-5",
      "claude-opus-5-5",
      "claude-haiku-4-5",
      "claude-fable-5-1",
    ]);
    expect(screen.queryByRole("button", { name: "Opus" })).toBeNull();
  });

  test("blank is Default, in Claude Code's words, and names its current model when it is known", async () => {
    await renderSetup();
    expect(field().placeholder).toBe("Default (Claude Code picks)");
    expect(screen.getByText("Default (Claude Code picks)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Default" })).toHaveAttribute("aria-pressed", "true");
  });

  test("a model Claude Code's own settings name shows as its current one", async () => {
    detectAgents.mockResolvedValue([claude({ cliDefault: "sonnet" }), codex()]);
    await renderSetup();
    expect(field().placeholder).toBe("Default (Claude Code picks, currently Sonnet)");

    expect(defaultModelText("Claude Code", claude({ cliDefault: "claude-new-6" }))).toBe(
      "Default (Claude Code picks, currently claude-new-6)",
    );
  });

  test("a button saves its model, and Default saves a blank one", async () => {
    agentChoice.mockResolvedValue({ harness: "claude-code", model: "sonnet", binaryPath: null });
    await renderSetup();
    expect(screen.getByRole("button", { name: "Sonnet" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Sonnet: balanced, good default for notes")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Haiku" }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(expect.objectContaining({ model: "haiku" })),
    );

    fireEvent.click(screen.getByRole("button", { name: "Default" }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(expect.objectContaining({ model: "" })),
    );
    expect(field().value).toBe("");
  });

  test("a stored opus shows as Opus picked in the dropdown, with no button pressed", async () => {
    agentChoice.mockResolvedValue({ harness: "claude-code", model: "opus", binaryPath: null });
    await renderSetup();

    expect(dropdown().value).toBe("opus");
    expect(field().value).toBe("opus");
    for (const name of ["Default", "Sonnet", "Haiku"]) {
      expect(screen.getByRole("button", { name })).toHaveAttribute("aria-pressed", "false");
    }
  });

  test("picking from the dropdown saves that model at once", async () => {
    await renderSetup();
    fireEvent.change(dropdown(), { target: { value: "claude-opus-5-5" } });
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenCalledWith({
        harness: "claude-code",
        model: "claude-opus-5-5",
        binaryPath: null,
      }),
    );
    expect(field().value).toBe("claude-opus-5-5");
    expect(dropdown().value).toBe("claude-opus-5-5");
  });

  test("a name the list does not have is still saved as typed", async () => {
    await renderSetup();
    fireEvent.change(field(), { target: { value: "  claude-next-7  " } });
    fireEvent.keyDown(field(), { key: "Enter" });
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(
        expect.objectContaining({ model: "claude-next-7" }),
      ),
    );
    expect(dropdown().value).toBe("");
    for (const name of ["Default", "Sonnet", "Haiku"]) {
      expect(screen.getByRole("button", { name })).toHaveAttribute("aria-pressed", "false");
    }
  });

  test("Test runs with the model picked", async () => {
    agentChoice.mockResolvedValue({ harness: "claude-code", model: "haiku", binaryPath: null });
    await renderSetup();
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    await waitFor(() =>
      expect(testAgent).toHaveBeenCalledWith({
        harness: "claude-code",
        model: "haiku",
        binaryPath: null,
      }),
    );
  });

  test("Codex gets Default and its first two models as buttons, the rest in the dropdown", async () => {
    agentChoice.mockResolvedValue({ harness: "codex", model: "", binaryPath: null });
    detectAgents.mockResolvedValue([claude(), codex()]);
    await renderSetup();

    expect(field().placeholder).toBe("Default (Codex picks)");
    expect(chips()).toEqual(["Default", "gpt-5.6-sol", "gpt-5.6-terra"]);
    const options = within(dropdown())
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(options).toEqual(["More models…", "gpt-5.6-luna", "gpt-5.5"]);
  });

  test("an agent with no models offers Default only, and free text", async () => {
    agentChoice.mockResolvedValue({ harness: "codex", model: "", binaryPath: null });
    detectAgents.mockResolvedValue([claude(), codex({ models: [] })]);
    await renderSetup();

    expect(chips()).toEqual(["Default"]);
    expect(screen.queryByRole("combobox", { name: "More models" })).toBeNull();
    expect(modelChoices([])).toEqual({ chips: [], more: [] });
  });
});
