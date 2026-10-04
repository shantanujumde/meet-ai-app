#!/usr/bin/env node
// meet-ai — WCAG contrast check for the design tokens (TUR-102).
//
// Reads tokens.css, works out every token's colour the way the browser does
// (the cascade, `var()`, `color-mix()` in sRGB with premultiplied alpha), and
// checks each text colour on each surface it sits on, in light, dark, with
// Increase Contrast, with Reduce Transparency and with the glass switch off.
// See-through surfaces are composed over a known backdrop first, because a
// contrast ratio against an alpha colour means nothing.
//
//   node design-system/meet-ai/contrast.mjs          a table, exit 1 on a fail
//   node design-system/meet-ai/contrast.mjs --json   the rows, for the test
//
// src/test/contrast.test.ts runs it, so CI fails when a token change breaks AA.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const TOKENS = readFileSync(join(HERE, "tokens.css"), "utf8");

// --- parsing ------------------------------------------------------------------

/** Every `--name: value;` with the selector and `@media` it sits under. */
function declarations(css) {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const found = [];
  const stack = []; // { kind: "media" | "rule", head }
  let head = "";
  let order = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "{") {
      const h = head.trim();
      stack.push({ kind: h.startsWith("@") ? "media" : "rule", head: h });
      head = "";
    } else if (ch === "}") {
      stack.pop();
      head = "";
    } else if (ch === ";") {
      const decl = head.trim();
      head = "";
      const rule = [...stack].reverse().find((s) => s.kind === "rule");
      const match = /^(--[\w-]+)\s*:\s*([\s\S]+)$/.exec(decl);
      if (rule && match) {
        found.push({
          name: match[1],
          value: match[2].trim(),
          selector: rule.head,
          media: stack.filter((s) => s.kind === "media").map((s) => s.head),
          order: order++,
        });
      }
    } else {
      head += ch;
    }
  }
  return found;
}

/** Does `@media` text hold in this environment? Unknown queries never do. */
function mediaHolds(media, env) {
  return media.every((query) => {
    if (/prefers-contrast:\s*more/.test(query)) return env.contrast;
    if (/prefers-reduced-transparency:\s*reduce/.test(query)) return env.reduceTransparency;
    return false;
  });
}

/**
 * The specificity this selector list matches with here, or -1. Only the
 * shapes tokens.css uses: `:root`, `:root[data-theme="dark"]`,
 * `:root[data-glass="off"]`. Anything else (the `data-os` rule) is not macOS.
 */
function matchSpecificity(selector, env) {
  let best = -1;
  for (const part of selector.split(",").map((p) => p.trim())) {
    if (part === ":root") best = Math.max(best, 1);
    else if (part === ':root[data-theme="dark"]' && env.theme === "dark") best = Math.max(best, 2);
    else if (part === ':root[data-glass="off"]' && env.glassOff) best = Math.max(best, 2);
  }
  return best;
}

/** The winning declaration of every custom property in `env`. */
function cascade(decls, env) {
  const won = new Map();
  for (const decl of decls) {
    if (!mediaHolds(decl.media, env)) continue;
    const spec = matchSpecificity(decl.selector, env);
    if (spec < 0) continue;
    const prev = won.get(decl.name);
    if (!prev || spec > prev.spec || (spec === prev.spec && decl.order > prev.order)) {
      won.set(decl.name, { spec, order: decl.order, value: decl.value });
    }
  }
  return new Map([...won].map(([name, { value }]) => [name, value]));
}

// --- colour -------------------------------------------------------------------

/** Split on top-level commas. */
function args(text) {
  const out = [];
  let depth = 0;
  let cur = "";
  for (const ch of text) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (ch === "," && depth === 0) {
      out.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

/** A colour as [r, g, b, a], r g b in 0–255 and a in 0–1. */
function color(value, vars, seen = new Set()) {
  const v = value.trim();
  if (v === "transparent") return [0, 0, 0, 0];
  if (v === "black") return [0, 0, 0, 1];
  if (v === "white") return [255, 255, 255, 1];
  let m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(v);
  if (m) {
    const hex = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
    return [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16)).concat(1);
  }
  m = /^rgba?\((.*)\)$/i.exec(v);
  if (m) {
    const [r, g, b, a = "1"] = args(m[1]);
    return [Number(r), Number(g), Number(b), Number(a)];
  }
  m = /^var\((--[\w-]+)\)$/.exec(v);
  if (m) {
    if (seen.has(m[1])) throw new Error(`cycle at ${m[1]}`);
    if (!vars.has(m[1])) throw new Error(`no value for ${m[1]}`);
    return color(vars.get(m[1]), vars, new Set([...seen, m[1]]));
  }
  m = /^color-mix\((.*)\)$/i.exec(v);
  if (m) {
    const [space, a, b] = args(m[1]);
    if (space !== "in srgb") throw new Error(`only srgb mixes: ${v}`);
    const part = (p) => {
      const pm = /^(.*?)\s+(\d+(?:\.\d+)?)%$/.exec(p);
      return pm ? [pm[1], Number(pm[2]) / 100] : [p, null];
    };
    let [ca, pa] = part(a);
    let [cb, pb] = part(b);
    if (pa === null && pb === null) pa = pb = 0.5;
    else if (pa === null) pa = 1 - pb;
    else if (pb === null) pb = 1 - pa;
    const x = color(ca, vars, seen);
    const y = color(cb, vars, seen);
    // Premultiplied, as CSS Color 5 specifies.
    const alpha = x[3] * pa + y[3] * pb;
    if (alpha === 0) return [0, 0, 0, 0];
    const ch = (i) => (x[i] * x[3] * pa + y[i] * y[3] * pb) / alpha;
    return [ch(0), ch(1), ch(2), alpha];
  }
  throw new Error(`cannot read colour: ${v}`);
}

/** `top` painted over `bottom`. */
function over(top, bottom) {
  const a = top[3] + bottom[3] * (1 - top[3]);
  if (a === 0) return [0, 0, 0, 0];
  const ch = (i) => (top[i] * top[3] + bottom[i] * bottom[3] * (1 - top[3])) / a;
  return [ch(0), ch(1), ch(2), a];
}

function luminance([r, g, b]) {
  const lin = (c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(fg, bg) {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((p, q) => q - p);
  return (hi + 0.05) / (lo + 0.05);
}

// --- what is checked ------------------------------------------------------------

// Behind the see-through sidebar is the native window material, and behind
// that the desktop. Neither is known, so the sidebar is composed over a
// mid-grey: darker than the light material and lighter than the dark one
// ever get. The popup (TUR-100) floats over anything, so both black and white.
const MID_GREY = "#808080";

/** A surface: the layers from the bottom up, each a colour or a token. */
const S = {
  canvas: ["var(--surface-canvas)"],
  content: ["var(--surface-canvas)", "var(--surface-content)"],
  card: ["var(--surface-canvas)", "var(--surface-card)"],
  "button on card": ["var(--surface-canvas)", "var(--surface-card)", "var(--surface-control)"],
  "button hover on card": [
    "var(--surface-canvas)",
    "var(--surface-card)",
    "var(--surface-control-hover)",
  ],
  "button on canvas": ["var(--surface-canvas)", "var(--surface-control)"],
  "field on card": ["var(--surface-canvas)", "var(--surface-card)", "var(--surface-glass-raised)"],
  "sunken on card": ["var(--surface-canvas)", "var(--surface-card)", "var(--surface-glass-sunken)"],
  "raised glass on canvas": ["var(--surface-canvas)", "var(--surface-glass-raised)"],
  "chosen tile on card": ["var(--surface-canvas)", "var(--surface-card)", "var(--accent-glass)"],
  sidebar: [MID_GREY, "var(--surface-sidebar)"],
  "sidebar row hover": [MID_GREY, "var(--surface-sidebar)", "var(--surface-control)"],
  "sidebar solid": ["var(--surface-sidebar-solid)"],
  "popup over black": ["#000000", "var(--surface-popup)"],
  "popup over white": ["#ffffff", "var(--surface-popup)"],
  "warning banner": [
    "var(--surface-canvas)",
    "color-mix(in srgb, var(--status-warning) 12%, transparent)",
  ],
  "danger banner": [
    "var(--surface-canvas)",
    "color-mix(in srgb, var(--status-danger) 12%, transparent)",
  ],
  "error state": [
    "var(--surface-canvas)",
    "color-mix(in srgb, var(--status-danger) 6%, transparent)",
  ],
  "security state": [
    "var(--surface-canvas)",
    "color-mix(in srgb, var(--status-warning) 8%, transparent)",
  ],
  "danger icon square on card": [
    "var(--surface-canvas)",
    "var(--surface-card)",
    "color-mix(in srgb, var(--status-danger-text) 12%, transparent)",
  ],
  "warning icon square on card": [
    "var(--surface-canvas)",
    "var(--surface-card)",
    "color-mix(in srgb, var(--status-warning-text) 14%, transparent)",
  ],
  "accent fill": ["var(--accent-fill)"],
  "accent hover": ["var(--accent-hover)"],
  "danger fill": ["var(--status-danger-fill)"],
};

const TEXT = ["primary", "secondary", "tertiary"].map((t) => `var(--text-${t})`);
const STATUS_TEXT = ["danger", "warning", "success", "recording"].map(
  (t) => `var(--status-${t}-text)`,
);

/** [text, surface, minimum, why]. 4.5 is AA for body text; 3 for graphics. */
const PAIRS = [
  ...Object.keys(S)
    .filter(
      (s) =>
        !/fill|hover$|icon square/.test(s) ||
        s === "sidebar row hover" ||
        s === "button hover on card",
    )
    .flatMap((s) => TEXT.map((t) => [t, s, 4.5, "text"])),
  ...["canvas", "content", "card", "chosen tile on card", "sidebar"].map((s) => [
    "var(--accent-text)",
    s,
    4.5,
    "accent as text",
  ]),
  ...["canvas", "content", "card", "sunken on card", "popup over black", "popup over white"].flatMap(
    (s) => STATUS_TEXT.map((t) => [t, s, 4.5, "status word"]),
  ),
  ...["sidebar"].map((s) => ["var(--status-recording-text)", s, 4.5, "status word"]),
  ...["accent fill", "accent hover", "danger fill"].map((s) => [
    "var(--text-on-accent)",
    s,
    4.5,
    "white on a filled control",
  ]),
  ...["card", "canvas"].map((s) => ["var(--accent)", s, 3, "switch on, chosen outline"]),
  ...["sidebar", "card", "chosen tile on card"].map((s) => [
    "var(--accent-text)",
    s,
    3,
    "row and sidebar icons",
  ]),
  ["var(--status-danger-text)", "danger icon square on card", 3, "error icon"],
  ["var(--status-warning-text)", "warning icon square on card", 3, "warning icon"],
  ...["card", "canvas"].flatMap((s) =>
    ["var(--speaker-1)", "var(--speaker-2)"].map((t) => [t, s, 3, "speaker dot"]),
  ),
];

const ENVS = [
  { name: "light", theme: "light" },
  { name: "dark", theme: "dark" },
  { name: "light, Increase Contrast", theme: "light", contrast: true },
  { name: "dark, Increase Contrast", theme: "dark", contrast: true },
  { name: "light, Reduce Transparency", theme: "light", reduceTransparency: true },
  { name: "dark, Reduce Transparency", theme: "dark", reduceTransparency: true },
  { name: "light, glass off", theme: "light", glassOff: true },
  { name: "dark, glass off", theme: "dark", glassOff: true },
];

export function check(css = TOKENS) {
  const decls = declarations(css);
  const rows = [];
  for (const env of ENVS) {
    const vars = cascade(decls, env);
    for (const [text, surface, min, why] of PAIRS) {
      const bg = S[surface].map((layer) => color(layer, vars)).reduce((acc, layer) => over(layer, acc));
      const fg = over(color(text, vars), bg);
      const value = ratio(fg, bg);
      rows.push({
        env: env.name,
        text: text.replace(/^var\((.*)\)$/, "$1"),
        surface,
        ratio: Math.round(value * 100) / 100,
        min,
        why,
        pass: value >= min,
      });
    }
  }
  return rows;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const rows = check();
  if (process.argv.includes("--json")) {
    process.stdout.write(JSON.stringify(rows));
  } else {
    for (const env of ENVS) {
      const mine = rows.filter((r) => r.env === env.name);
      const lowest = mine.filter((r) => r.min === 4.5).reduce((a, r) => (r.ratio < a.ratio ? r : a));
      console.log(`\n${env.name}: lowest text pair ${lowest.ratio}:1 (${lowest.text} on ${lowest.surface})`);
      for (const r of mine) {
        const flag = r.pass ? "  " : "!!";
        console.log(`${flag} ${r.ratio.toFixed(2).padStart(6)}:1  (min ${r.min})  ${r.text} on ${r.surface}`);
      }
    }
    const fails = rows.filter((r) => !r.pass);
    console.log(`\n${rows.length} pairs, ${fails.length} below the minimum.`);
    process.exit(fails.length === 0 ? 0 : 1);
  }
}
