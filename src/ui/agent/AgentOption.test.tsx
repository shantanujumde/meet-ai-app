import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, test, vi } from "vitest";
import { Pill } from "@/ui/primitives";
import { AgentOption } from "./AgentOption";

/**
 * The agent row (TUR-73): the whole row — name, detail line, the space up to
 * the status pill — picks the agent, and the arrow keys move between agents.
 */

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn(async (_text: string) => {}) }));

function Picker({ onPick }: { onPick: (id: string) => void }) {
  const [picked, setPicked] = useState<string>("claude-code");
  const pick = (id: string) => () => {
    setPicked(id);
    onPick(id);
  };
  return (
    <fieldset>
      <legend>Who writes your notes</legend>
      <AgentOption
        group="agents"
        value="claude-code"
        name="Claude Code"
        detail="Version 2.1.286 · /usr/local/bin/claude"
        checked={picked === "claude-code"}
        onPick={pick("claude-code")}
        status={<Pill tone="ok">Signed in</Pill>}
      />
      <AgentOption
        group="agents"
        value="codex"
        name="Codex"
        detail="Not installed, or not where meet-ai looked"
        checked={picked === "codex"}
        onPick={pick("codex")}
        status={<Pill>Not found</Pill>}
      />
      <AgentOption
        group="agents"
        value="none"
        name="None, I'll copy the prompt"
        detail="After each call, Copy prompt puts the notes prompt on the clipboard"
        checked={picked === "none"}
        onPick={pick("none")}
      />
    </fieldset>
  );
}

describe("AgentOption", () => {
  test("clicking the agent's name picks it", async () => {
    const onPick = vi.fn();
    const user = userEvent.setup();
    render(<Picker onPick={onPick} />);

    await user.click(screen.getByText("Codex"));
    expect(onPick).toHaveBeenLastCalledWith("codex");
    expect(screen.getByRole("radio", { name: /Codex/ })).toBeChecked();
  });

  test("clicking the detail line picks it", async () => {
    const onPick = vi.fn();
    const user = userEvent.setup();
    render(<Picker onPick={onPick} />);

    await user.click(screen.getByText(/Copy prompt puts the notes prompt/));
    expect(onPick).toHaveBeenLastCalledWith("none");
    expect(screen.getByRole("radio", { name: /None, I'll copy the prompt/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /Claude Code/ })).not.toBeChecked();
  });

  test("the arrow keys move between agents", async () => {
    const onPick = vi.fn();
    const user = userEvent.setup();
    render(<Picker onPick={onPick} />);

    await user.tab();
    expect(screen.getByRole("radio", { name: /Claude Code/ })).toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("radio", { name: /Codex/ })).toBeChecked();
    expect(onPick).toHaveBeenLastCalledWith("codex");

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("radio", { name: /None/ })).toHaveFocus();
    expect(onPick).toHaveBeenLastCalledWith("none");

    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("radio", { name: /Codex/ })).toBeChecked();
  });

  test("the status pill stays outside the radio's name", () => {
    render(<Picker onPick={() => {}} />);
    const radio = screen.getByRole("radio", { name: /Claude Code/ });
    expect(radio).not.toHaveAccessibleName(/Signed in/);
    expect(radio).toHaveAccessibleName(/Version 2\.1\.286/);
  });
});
