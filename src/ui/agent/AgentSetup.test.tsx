import { open } from "@tauri-apps/plugin-dialog";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { AgentCli, AgentTestResult } from "@/ipc/types";
import { CLAUDE_MODELS, ipc } from "@/test/ipcMock";
import { AgentSetup } from "./AgentSetup";

/**
 * The agent card (SPEC A11, the Setup row), against a stubbed backend: what
 * each detected state looks like, that every change saves at once, and the
 * Test button's answer either way.
 */

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), ask: vi.fn() }));

const { agentChoice, detectAgents, saveAgentChoice, testAgent } = ipc;
const pickFile = vi.mocked(open);

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
    state: "missing",
    path: null,
    version: null,
    signInCommand: "codex login",
    models: [],
    cliDefault: null,
    canTest: false,
    ...overrides,
  };
}

/** The radio row for one option, to look inside it. */
function option(name: RegExp): HTMLElement {
  const radio = screen.getByRole("radio", { name });
  const row = radio.closest("label")?.parentElement?.parentElement;
  if (!row) throw new Error("no row around the radio");
  return row;
}

/** Render, and wait for config and detection to answer. */
async function renderSetup() {
  render(<AgentSetup />);
  await waitFor(() => expect(detectAgents).toHaveBeenCalled());
  await waitFor(() => expect(screen.queryByText("Checking…")).toBeNull());
}

describe("AgentSetup", () => {
  test("each row says Checking… while detection runs", async () => {
    detectAgents.mockReturnValue(new Promise(() => {}));
    render(<AgentSetup />);

    await waitFor(() => expect(detectAgents).toHaveBeenCalled());
    expect(within(option(/Claude Code/)).getByText("Checking…")).toBeInTheDocument();
    expect(within(option(/Codex/)).getByText("Checking…")).toBeInTheDocument();
  });

  test("a signed-in agent says so, with its version, and is the one picked", async () => {
    await renderSetup();

    const row = option(/Claude Code/);
    expect(within(row).getByText("Signed in")).toBeInTheDocument();
    expect(within(row).getByText("Version 2.1.286 · /usr/local/bin/claude")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Claude Code/ })).toBeChecked();
  });

  test("a signed-out agent shows the command that signs it in, and Check again looks again", async () => {
    detectAgents.mockResolvedValue([claude({ state: "signed-out" }), codex()]);
    await renderSetup();

    const row = option(/Claude Code/);
    expect(within(row).getByText("Installed, not signed in")).toBeInTheDocument();
    expect(within(row).getByText("claude auth login")).toBeInTheDocument();
    expect(within(row).getByText("Run this in Terminal, then check again:")).toBeInTheDocument();

    detectAgents.mockResolvedValue([claude(), codex()]);
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await within(row).findByText("Signed in")).toBeInTheDocument();
    expect(detectAgents).toHaveBeenCalledTimes(2);
    expect(within(row).queryByText("claude auth login")).toBeNull();
  });

  test("an agent that is not installed says Not found", async () => {
    await renderSetup();
    expect(within(option(/Codex/)).getByText("Not found")).toBeInTheDocument();
  });

  test("picking None saves harness none, and the privacy sentence goes away", async () => {
    await renderSetup();
    expect(screen.getByText(/sends the transcript — never the audio — to Anthropic/)).toBeTruthy();

    fireEvent.click(screen.getByRole("radio", { name: /None, I'll copy the prompt/ }));

    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenCalledWith({
        harness: "none",
        model: "",
        binaryPath: null,
      }),
    );
    expect(screen.getByRole("radio", { name: /None/ })).toBeChecked();
    expect(screen.queryByText(/sends the transcript/)).toBeNull();
    expect(screen.getByText(/Nothing is sent anywhere/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Test" })).toBeDisabled();
  });

  test("the privacy sentence names the provider and the user's own account", async () => {
    await renderSetup();
    expect(
      screen.getByText(
        "When a call ends, meet-ai sends the transcript — never the audio — to Anthropic through your own Claude Code account. You can turn this off for any meeting.",
      ),
    ).toBeInTheDocument();
  });

  test("the model starts on Default, and free text saves on blur, not per key", async () => {
    await renderSetup();
    const field = screen.getByRole("textbox", { name: /Model/ }) as HTMLInputElement;
    expect(field.value).toBe("");
    expect(field.placeholder).toBe("Default (Claude Code picks)");
    expect(screen.getByRole("button", { name: "Default" })).toHaveAttribute("aria-pressed", "true");

    fireEvent.change(field, { target: { value: "claude-opus-4-1" } });
    expect(saveAgentChoice).not.toHaveBeenCalled();

    fireEvent.blur(field);
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenCalledWith({
        harness: "claude-code",
        model: "claude-opus-4-1",
        binaryPath: null,
      }),
    );
    expect(saveAgentChoice).toHaveBeenCalledTimes(1);
  });

  test("Enter saves the model, and a suggestion saves at once", async () => {
    await renderSetup();
    const field = screen.getByRole("textbox", { name: /Model/ });

    fireEvent.change(field, { target: { value: "haiku" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(expect.objectContaining({ model: "haiku" })),
    );

    fireEvent.click(screen.getByRole("button", { name: "Sonnet" }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(
        expect.objectContaining({ model: "sonnet" }),
      ),
    );
  });

  test("Choose file… saves the path and looks again; Find automatically clears it", async () => {
    pickFile.mockResolvedValue("/opt/tools/claude");
    await renderSetup();

    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));

    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenCalledWith({
        harness: "claude-code",
        model: "",
        binaryPath: "/opt/tools/claude",
      }),
    );
    expect(pickFile).toHaveBeenCalledWith(
      expect.objectContaining({ directory: false, multiple: false }),
    );
    await waitFor(() =>
      expect(detectAgents).toHaveBeenLastCalledWith(
        expect.objectContaining({ binaryPath: "/opt/tools/claude" }),
      ),
    );
    expect(await screen.findByText("/opt/tools/claude (chosen by you)")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Find automatically" }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(
        expect.objectContaining({ binaryPath: null }),
      ),
    );
  });

  test("Test shows what the sample run wrote", async () => {
    await renderSetup();

    let finish: (result: AgentTestResult) => void = () => {};
    testAgent.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(await screen.findByText("Testing… this can take up to a minute.")).toBeInTheDocument();
    expect(testAgent).toHaveBeenCalledWith({
      harness: "claude-code",
      model: "",
      binaryPath: null,
    });

    await act(async () =>
      finish({
        summary: "The beta ships Friday.",
        decisions: ["Ship the beta Friday"],
        openQuestions: ["Do we need legal sign-off?"],
        tasks: [{ title: "Write the release notes", owner: "Ben", due: "Thursday" }],
        seconds: 9.6,
      }),
    );

    expect(
      screen.getByText("It works. Claude Code wrote these notes in 10 seconds."),
    ).toBeInTheDocument();
    expect(screen.getByText("The beta ships Friday.")).toBeInTheDocument();
    expect(screen.getByText("Write the release notes · Ben · due Thursday")).toBeInTheDocument();
    expect(screen.getByText("Do we need legal sign-off?")).toBeInTheDocument();
    expect(screen.queryByText(/this can take up to a minute/)).toBeNull();
  });

  test("a failed Test shows the agent's own reason", async () => {
    testAgent.mockRejectedValue({
      domain: "app",
      kind: "agent-not-signed-in",
      message: "Claude Code is not signed in. Run `claude auth login`.",
    });
    await renderSetup();

    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    expect(
      await screen.findByText("Claude Code is not signed in. Run `claude auth login`."),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  test("Test is off for an agent that is not installed", async () => {
    agentChoice.mockResolvedValue({ harness: "codex", model: "", binaryPath: null });
    await renderSetup();

    expect(screen.getByRole("button", { name: "Test" })).toBeDisabled();
    expect(
      screen.getByText("Install Codex, or choose where it is above, to test it."),
    ).toBeInTheDocument();
  });

  test("a ready Codex can be picked and tested, on its own default model", async () => {
    detectAgents.mockResolvedValue([
      claude(),
      codex({
        state: "ready",
        path: "/opt/homebrew/bin/codex",
        version: "0.50.0",
        models: [{ name: "gpt-5-codex", label: "gpt-5-codex", note: null }],
        canTest: true,
      }),
    ]);
    await renderSetup();

    fireEvent.click(screen.getByRole("radio", { name: /Codex/ }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenCalledWith({
        harness: "codex",
        model: "",
        binaryPath: null,
      }),
    );
    const field = screen.getByRole("textbox", { name: /Model/ }) as HTMLInputElement;
    expect(field.value).toBe("");
    expect(field.placeholder).toBe("Default (Codex picks)");
    expect(screen.getByText(/to OpenAI through your own Codex account/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    expect(
      await screen.findByText("It works. Codex wrote these notes in 10 seconds."),
    ).toBeInTheDocument();
    expect(testAgent).toHaveBeenCalledWith({ harness: "codex", model: "", binaryPath: null });
  });

  test("a config naming an unknown agent is shown, and picking one fixes it", async () => {
    agentChoice.mockRejectedValue({
      domain: "app",
      kind: "unknown-harness",
      message: 'config.jsonc: agent.harness "gemini" is not one meet-ai knows.',
    });
    await renderSetup();

    expect(
      screen.getByText('config.jsonc: agent.harness "gemini" is not one meet-ai knows.'),
    ).toBeInTheDocument();
    expect(screen.getByText("Picking an agent below fixes this.")).toBeInTheDocument();
    for (const radio of screen.getAllByRole("radio")) expect(radio).not.toBeChecked();

    fireEvent.click(screen.getByRole("radio", { name: /Claude Code/ }));

    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenCalledWith({
        harness: "claude-code",
        model: "",
        binaryPath: null,
      }),
    );
    await waitFor(() => expect(screen.queryByText(/is not one meet-ai knows/)).toBeNull());
    expect(screen.getByRole("radio", { name: /Claude Code/ })).toBeChecked();
  });

  test("a save that fails puts the old pick back and says why", async () => {
    saveAgentChoice.mockRejectedValue({
      domain: "app",
      kind: "no-config-dir",
      message: "No config folder.",
    });
    await renderSetup();

    fireEvent.click(screen.getByRole("radio", { name: /None/ }));

    expect(await screen.findByText("No config folder.")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Claude Code/ })).toBeChecked();
  });

  test("a slow, older model save cannot overwrite a newer one", async () => {
    await renderSetup();
    let finishFirst: (value: { harness: "claude-code"; model: string; binaryPath: null }) => void =
      () => {};
    saveAgentChoice.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishFirst = resolve;
        }),
    );
    const field = screen.getByRole("textbox", { name: /Model/ }) as HTMLInputElement;
    fireEvent.change(field, { target: { value: "opus" } });
    fireEvent.blur(field);
    fireEvent.click(screen.getByRole("button", { name: "Sonnet" }));
    await waitFor(() => expect(saveAgentChoice).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(field.value).toBe("sonnet"));

    await act(async () => finishFirst({ harness: "claude-code", model: "opus", binaryPath: null }));
    expect(field.value).toBe("sonnet");
    expect(screen.getByRole("button", { name: "Sonnet" })).toHaveAttribute("aria-pressed", "true");
  });

  test("None and back to Claude Code finds its model again", async () => {
    await renderSetup();
    fireEvent.click(screen.getByRole("button", { name: "Sonnet" }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(
        expect.objectContaining({ model: "sonnet" }),
      ),
    );
    fireEvent.click(screen.getByRole("radio", { name: /None, I'll copy the prompt/ }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith(
        expect.objectContaining({ harness: "none" }),
      ),
    );
    fireEvent.click(screen.getByRole("radio", { name: /Claude Code/ }));
    await waitFor(() =>
      expect(saveAgentChoice).toHaveBeenLastCalledWith({
        harness: "claude-code",
        model: "sonnet",
        binaryPath: null,
      }),
    );
  });
});
