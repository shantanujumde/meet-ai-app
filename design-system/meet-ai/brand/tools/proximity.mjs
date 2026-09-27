/* =============================================================================
   meet-ai brand — prior-art proximity probe: the viewfinder / focus-target class
   Raised in review (TUR-12): `[ · ]` may share a silhouette with the generic
   camera-focus glyph — Apple's `viewfinder` / `dot.viewfinder` SF Symbols and
   the scan-to-capture target used across iOS and document scanners.

   This is a *meaning* risk, not a trademark one: nobody owns a viewfinder. The
   question is what a user's first read is at 16px with colour stripped, which
   is the one context this mark was optimised for.

   The reference glyphs below are DRAWN HERE from the generic description of
   the class (four corner marks, optional centre dot). They are not traced from
   Apple's artwork and no Apple asset is copied into this repo.

     node proximity.mjs && ./render-proximity.sh
   ============================================================================= */
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { BRACKETS, MENUBAR, bracketPaths, f } from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const STAGE = join(HERE, ".render");
mkdirSync(STAGE, { recursive: true });

/* -----------------------------------------------------------------------------
   The reference class. Four corner marks on a square, arms equal on both axes.
   `arm` is the run of each corner mark; the gap in each side is what is left.
-------------------------------------------------------------------------- */
function viewfinder({ half = 34, arm = 13, r = 6, stroke = 11, dot = 0 }) {
  const a = 50 - half;
  const b = 50 + half;
  const c = [
    `M${a},${f(a + arm)} V${f(a + r)} A${r},${r} 0 0 1 ${f(a + r)},${a} H${f(a + arm)}`,
    `M${f(b - arm)},${a} H${f(b - r)} A${r},${r} 0 0 1 ${b},${f(a + r)} V${f(a + arm)}`,
    `M${b},${f(b - arm)} V${f(b - r)} A${r},${r} 0 0 0 ${f(b - r)},${b} H${f(b - arm)}`,
    `M${f(a + arm)},${b} H${f(a + r)} A${r},${r} 0 0 0 ${a},${f(b - r)} V${f(b - arm)}`,
  ];
  return { paths: c, stroke, dot };
}

/* A bracket pair, from a BRACKETS-shaped config. */
function pair(g) {
  const { left, right } = bracketPaths(g);
  return { paths: [left, right], stroke: g.stroke, dot: g.dot.r, dotAt: g.dot };
}

/* -----------------------------------------------------------------------------
   Variants. V0 is what is on the branch today. The rest move the two knobs the
   review asked about — gutter width, and the aspect of the enclosure — so the
   question is answered against rendered art rather than argued.
-------------------------------------------------------------------------- */
const B = BRACKETS;
const V = [
  ["ref-vf", "generic viewfinder", viewfinder({ dot: 0 })],
  ["ref-vfdot", "generic focus target", viewfinder({ dot: 10 })],
  ["v0", "V0 — on the branch", pair(B)],
  ["v1", "V1 — gutter +10", pair({ ...B, armL: 31, armR: 69 })],
  ["v2", "V2 — gutter −10 (longer arms)", pair({ ...B, armL: 41, armR: 59 })],
  ["v3", "V3 — portrait (taller, narrower)", pair({ ...B, stemL: 21, stemR: 79, top: 15, bottom: 85 })],
  ["v4", "V4 — portrait + longer arms", pair({ ...B, stemL: 21, stemR: 79, top: 15, bottom: 85, armL: 43, armR: 57 })],
];

function svg(v, fg, size) {
  const s = `fill="none" stroke="${fg}" stroke-width="${v.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  const dot = v.dot
    ? `<circle cx="${v.dotAt?.cx ?? 50}" cy="${v.dotAt?.cy ?? 50}" r="${v.dot}" fill="${fg}"/>`
    : "";
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="${size}" height="${size}">` +
    v.paths.map((d) => `<path d="${d}" ${s}/>`).join("") +
    dot +
    `</svg>`
  );
}

/* -----------------------------------------------------------------------------
   16px pixel-grid row. The 100-unit art is not what ships at 16px — the menu
   bar template is drawn on the pixel grid — so the gutter question has to be
   asked again in those terms, with whole-pixel strokes.
-------------------------------------------------------------------------- */
const M = MENUBAR;
const PX = [
  ["p-ref", "focus target, 16px", null],
  ["p0", "V0 — 16px, gutter 4px", M],
  ["p1", "V1 — gutter 6px", { ...M, armL: 4, armR: 12 }],
  ["p2", "V2 — gutter 2px", { ...M, armL: 6, armR: 10 }],
];

function px16(g, fg) {
  if (!g) {
    // 16px focus target: 3px corner marks, 6px gaps, 2px dot.
    const s = `fill="none" stroke="${fg}" stroke-width="2" stroke-linecap="square"`;
    return (
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16" shape-rendering="crispEdges">` +
      `<path d="M2,6 V3 H5" ${s}/><path d="M11,3 H14 V6" ${s}/>` +
      `<path d="M14,10 V13 H11" ${s}/><path d="M5,13 H2 V10" ${s}/>` +
      `<rect x="7" y="7" width="2" height="2" fill="${fg}"/></svg>`
    );
  }
  const { left, right } = bracketPaths(g);
  const s = `fill="none" stroke="${fg}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">` +
    `<path d="${left}" ${s}/><path d="${right}" ${s}/>` +
    `<circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${fg}"/></svg>`
  );
}

/* -----------------------------------------------------------------------------
   The app icon at 16px is the tightest case: the mark sits inside a 14px
   squircle body, so it has ~12px to work in and the gutter is measured in
   1px steps. Drawn on white-in-dark-body, the way it actually ships.
-------------------------------------------------------------------------- */
import { ICON16, squirclePath } from "./geometry.mjs";
const I = ICON16;
const ICONS = [
  ["i0", "V0 — gutter 2px", I],
  ["i1", "V1 — gutter 4px", { ...I, armL: 5, armR: 11 }],
  ["i2", "V1b — gutter 4px, stems in", { ...I, stemL: 5, stemR: 11, armL: 6, armR: 10 }],
];

function icon16(g) {
  const { left, right } = bracketPaths(g);
  const s = `fill="none" stroke="#F4F5F7" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="16" height="16">` +
    `<path d="${squirclePath(8, 8, 7, 4.6, 64)}" fill="#1D1F25"/>` +
    `<path d="${left}" ${s}/><path d="${right}" ${s}/>` +
    `<circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="#FF8A3C"/></svg>`
  );
}

const cell = (label, body) =>
  `<div class="c"><div class="art">${body}</div><div class="l">${label}</div></div>`;

const row = (size, fg, bg) =>
  V.map(([, label, v]) => cell(label, svg(v, fg, size))).join("");

const pxrow = (fg, scale) =>
  PX.map(([, label, g]) =>
    cell(
      scale > 1 ? `${label}` : label,
      `<div style="zoom:${scale};image-rendering:pixelated">${px16(g, fg)}</div>`
    )
  ).join("");

const html = `<!doctype html><meta charset="utf-8"><style>
  :root { color-scheme: dark }
  body { margin:0; background:#8A8A92; font:13px/1.45 -apple-system,system-ui,sans-serif; color:#fff; padding:28px 32px 40px }
  h1 { font:600 19px/1.3 -apple-system,system-ui,sans-serif; margin:0 0 4px }
  p.sub { margin:0 0 22px; max-width:760px; color:#EDEDF2 }
  section { background:#1A1B20; border-radius:14px; padding:18px 20px 20px; margin:0 0 16px }
  section.light { background:#F2F2F5; color:#16181D }
  h2 { font:600 13px/1.3 -apple-system,system-ui,sans-serif; margin:0 0 3px; letter-spacing:.01em }
  .note { margin:0 0 14px; font-size:12px; opacity:.72 }
  .r { display:flex; gap:16px; align-items:flex-end; flex-wrap:wrap }
  .c { text-align:center }
  .art { display:flex; align-items:center; justify-content:center; min-height:132px }
  .l { margin-top:7px; font-size:10.5px; opacity:.66; max-width:118px }
  .sep { width:1px; align-self:stretch; background:currentColor; opacity:.18; margin:0 6px }
</style>
<h1>Proximity probe — the viewfinder / focus-target class</h1>
<p class="sub">Reference glyphs at the left of each row are drawn here from the generic description of the class
(four corner marks, optional centre dot). Nothing is traced from Apple artwork. The question is the first read at
16px in one colour, not at 512px in full colour.</p>

<section>
  <h2>128px, one colour on dark</h2>
  <p class="note">Silhouette only. Two continuous side stems versus four disjoint corner marks.</p>
  <div class="r">${row(128, "#F4F5F7")}</div>
</section>

<section class="light">
  <h2>128px, one colour on light</h2>
  <div class="r">${row(128, "#16181D")}</div>
</section>

<section>
  <h2>32px — the reductive test</h2>
  <p class="note">Where the structural difference either survives or collapses.</p>
  <div class="r">${row(32, "#F4F5F7")}</div>
</section>

<section>
  <h2>16px pixel grid — the menu bar, magnified 6×</h2>
  <p class="note">What actually ships at 16px is the pixel-grid template, not the 100-unit art scaled down.
  Gutter measured in whole pixels of white in the top and bottom edges.</p>
  <div class="r">${pxrow("#F4F5F7", 6)}</div>
</section>

<section>
  <h2>16px pixel grid — actual size</h2>
  <div class="r" style="align-items:center">${pxrow("#F4F5F7", 1)}</div>
</section>

<section>
  <h2>App icon at 16px — the tightest case, magnified 7×</h2>
  <p class="note">The mark has ~12px inside a 14px squircle body. Gutter here moves in single pixels.</p>
  <div class="r">${ICONS.map(([, l, g]) =>
    cell(l, `<div style="zoom:7;image-rendering:pixelated">${icon16(g)}</div>`)
  ).join("")}</div>
  <div class="r" style="align-items:center;margin-top:10px">${ICONS.map(([, l, g]) =>
    cell(l, icon16(g))
  ).join("")}</div>
</section>
`;

writeFileSync(join(STAGE, "proof-proximity.html"), html);
console.log("wrote .render/proof-proximity.html");
