import { expect, test } from "vitest";
import { showsCopyPrompt } from "./copyPrompt";

test.each([
  { harnessIsNone: true, cliFound: true, shows: true },
  { harnessIsNone: true, cliFound: false, shows: true },
  { harnessIsNone: false, cliFound: false, shows: true },
  { harnessIsNone: false, cliFound: true, shows: false },
])(
  "harness none: $harnessIsNone, CLI found: $cliFound → shows: $shows",
  ({ harnessIsNone, cliFound, shows }) => {
    expect(showsCopyPrompt({ harnessIsNone, cliFound })).toBe(shows);
  },
);

test("the CLI counts as found until something detects it", () => {
  expect(showsCopyPrompt({ harnessIsNone: false })).toBe(false);
  expect(showsCopyPrompt({ harnessIsNone: true })).toBe(true);
});
