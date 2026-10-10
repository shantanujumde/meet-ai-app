/** TUR-170: the switch's hint says when notes are written. */

import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { NOTES_SWITCH_LABEL, NotesSwitch } from "./NotesSwitch";

function hint(on: boolean, manual: boolean): HTMLElement {
  render(<NotesSwitch on={on} busy={false} error={null} manual={manual} onChange={() => {}} />);
  return screen.getByRole("switch", { name: NOTES_SWITCH_LABEL });
}

test("on, with notes after every call, says they come when the call ends", () => {
  expect(hint(true, false)).toHaveAccessibleDescription(
    "On: your agent writes notes from the transcript when the call ends.",
  );
});

test("on, with notes only on request, says how to ask for them", () => {
  expect(hint(true, true)).toHaveAccessibleDescription(
    "On: your agent writes notes from the transcript when you click Make notes now.",
  );
});

test("off says nothing is sent, whichever way notes run", () => {
  expect(hint(false, true)).toHaveAccessibleDescription(
    "Off: the transcript is not sent to your agent. For private calls.",
  );
});
