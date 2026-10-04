/// <reference types="node" />
/**
 * TUR-102: every text colour holds WCAG AA on every surface it sits on, in
 * light, dark, Increase Contrast, Reduce Transparency and with glass off.
 * The checker is `design-system/meet-ai/contrast.mjs`, which reads
 * `tokens.css` itself, so a token change that breaks a pair fails here.
 */

import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

type Row = {
  env: string;
  text: string;
  surface: string;
  ratio: number;
  min: number;
  pass: boolean;
};

// Vitest runs from the repo root.
const script = join(process.cwd(), "design-system", "meet-ai", "contrast.mjs");
const rows = JSON.parse(
  execFileSync(process.execPath, [script, "--json"], { encoding: "utf8" }),
) as Row[];

describe("design token contrast", () => {
  it("checks every environment", () => {
    const envs = new Set(rows.map((row) => row.env));
    for (const env of ["light", "dark", "light, Increase Contrast", "dark, Increase Contrast"]) {
      expect(envs).toContain(env);
    }
    expect(rows.length).toBeGreaterThan(500);
  });

  it("has no pair under its minimum", () => {
    const fails = rows
      .filter((row) => !row.pass)
      .map(
        (row) => `${row.env}: ${row.text} on ${row.surface} is ${row.ratio}:1, needs ${row.min}`,
      );
    expect(fails).toEqual([]);
  });

  it("keeps the record prompt's popup surface readable (TUR-100)", () => {
    const popup = rows.filter((row) => row.surface.startsWith("popup"));
    expect(popup.length).toBeGreaterThan(0);
    for (const row of popup) expect(row.ratio).toBeGreaterThanOrEqual(4.5);
  });
});
