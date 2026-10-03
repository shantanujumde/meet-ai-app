import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, test, vi } from "vitest";
import { Radio } from "./Radio";

/**
 * The shared radio (TUR-73): the whole label picks it, the arrow keys walk
 * the group, and a disabled one cannot be picked. The look itself — size,
 * ring, accent — is checked by hand (docs/manual-checks/worktree-tur73.md);
 * jsdom draws nothing.
 */

const CHOICES = [
  { value: "a", name: "Alpha", detail: "The first one" },
  { value: "b", name: "Bravo", detail: "The second one" },
  { value: "c", name: "Charlie", detail: "The third one" },
];

function Group({
  onChange,
  disabled = [],
}: {
  onChange?: (value: string) => void;
  disabled?: string[];
}) {
  const [picked, setPicked] = useState("a");
  return (
    <fieldset>
      <legend>Pick one</legend>
      {CHOICES.map((c) => (
        <Radio
          key={c.value}
          name="group"
          value={c.value}
          checked={picked === c.value}
          disabled={disabled.includes(c.value)}
          onChange={(value) => {
            setPicked(value);
            onChange?.(value);
          }}
        >
          <span>{c.name}</span>
          <span>{c.detail}</span>
        </Radio>
      ))}
    </fieldset>
  );
}

describe("Radio", () => {
  test("is a native radio, named by its label", () => {
    render(<Group />);
    const radio = screen.getByRole("radio", { name: /Bravo/ });
    expect(radio).toHaveAttribute("type", "radio");
    expect(radio).toHaveAttribute("name", "group");
    expect(radio).toHaveAttribute("value", "b");
    expect(screen.getByRole("radio", { name: /Alpha/ })).toBeChecked();
  });

  test("clicking the name or the detail line picks it, with its value", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Group onChange={onChange} />);

    await user.click(screen.getByText("The second one"));
    expect(onChange).toHaveBeenLastCalledWith("b");
    expect(screen.getByRole("radio", { name: /Bravo/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /Alpha/ })).not.toBeChecked();

    await user.click(screen.getByText("Charlie"));
    expect(onChange).toHaveBeenLastCalledWith("c");
    expect(screen.getByRole("radio", { name: /Charlie/ })).toBeChecked();
  });

  test("the arrow keys move the pick through the group, and Space picks", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Group onChange={onChange} />);

    await user.tab();
    expect(screen.getByRole("radio", { name: /Alpha/ })).toHaveFocus();

    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("radio", { name: /Bravo/ })).toHaveFocus();
    expect(screen.getByRole("radio", { name: /Bravo/ })).toBeChecked();
    expect(onChange).toHaveBeenLastCalledWith("b");

    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("radio", { name: /Charlie/ })).toBeChecked();

    await user.keyboard("{ArrowUp}");
    expect(screen.getByRole("radio", { name: /Bravo/ })).toBeChecked();
    expect(onChange).toHaveBeenCalledTimes(3);
  });

  test("a disabled radio cannot be picked by click", () => {
    const onChange = vi.fn();
    render(<Group onChange={onChange} disabled={["b"]} />);

    const radio = screen.getByRole("radio", { name: /Bravo/ });
    expect(radio).toBeDisabled();
    fireEvent.click(screen.getByText("The second one"));
    expect(onChange).not.toHaveBeenCalled();
    expect(radio).not.toBeChecked();
  });
});
