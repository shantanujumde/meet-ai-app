import { expect, test } from "vitest";
import { cn } from "./cn";

test("a type-scale size and a text colour are both kept", () => {
  // Without the theme names, tailwind-merge reads `text-body` as a colour and
  // drops whichever of the two comes first.
  expect(cn("text-body", "text-fg-secondary")).toBe("text-body text-fg-secondary");
  expect(cn("text-caption2 font-medium", "text-warning")).toBe(
    "text-caption2 font-medium text-warning",
  );
});

test("a later utility overrides an earlier one in the same group", () => {
  expect(cn("px-6 py-5", "p-0")).toBe("p-0");
  expect(cn("text-body", "text-footnote")).toBe("text-footnote");
  expect(cn("rounded-control", "rounded-capsule")).toBe("rounded-capsule");
  expect(cn("font-mono", "font-ui")).toBe("font-ui");
});
