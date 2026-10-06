import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, test } from "vitest";

/**
 * TUR-115: the meeting page must never scroll sideways. jsdom has no layout,
 * so this reads the rules in app.css that keep long words inside the column.
 */
const css = readFileSync(resolve(__dirname, "../app.css"), "utf8");

function rule(selector: string): string {
  const start = css.indexOf(`\n${selector} {`);
  expect(start).toBeGreaterThan(-1);
  return css.slice(start, css.indexOf("}", start));
}

describe("no sideways scroll (TUR-115)", () => {
  test("a transcript line's text column can shrink below its longest word", () => {
    expect(rule(".transcript__line")).toContain(
      "grid-template-columns: 7ch var(--transcript-speaker-w) minmax(0, 1fr);",
    );
  });

  test("transcript text breaks long URLs and tokens", () => {
    expect(rule(".transcript__text")).toContain("overflow-wrap: anywhere;");
  });

  test("the content column clips sideways overflow without a new scroll box", () => {
    expect(rule(".content")).toContain("overflow-x: clip;");
  });
});
