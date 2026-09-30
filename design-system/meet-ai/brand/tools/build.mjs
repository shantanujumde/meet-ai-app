/* =============================================================================
   meet-ai brand — asset build
   Generates every SVG master under ../ and writes a render manifest that
   render.sh turns into PNG / ICNS / ICO. Nothing under ../ is hand-edited.

     node build.mjs        # SVG masters + manifest
     ./render.sh           # rasters, icns, ico, favicons, src-tauri/icons
   ============================================================================= */
import { writeFileSync, readFileSync, mkdirSync, rmSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  PALETTE as P,
  TILE_GRADIENT as TG,
  MDOT,
  MDOT_SMALL,
  MENUBAR,
  ICON16,
  mdot,
  mdotGrid,
  pixelRuns,
  squirclePath,
  layoutWordmark,
  f,
} from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BRAND = resolve(HERE, "..");
const REPO = resolve(HERE, "..", "..", "..", "..");
const STAGE = join(HERE, ".render");
// Prune only what this script owns. `.render/` is shared: proof.mjs stages its
// HTML here too, so that relative <img> links to the rasters resolve under
// file://. Wiping the directory wholesale used to delete those pages out from
// under their render scripts — and because Chrome answers a missing file://
// URL with an error page rather than a non-zero exit, the result was a
// screenshot of the failure, committed as a proof (TUR-12).
mkdirSync(STAGE, { recursive: true });
for (const f of readdirSync(STAGE)) {
  if (
    /^(icon|mark|tray)-.*\.(html|png)$/.test(f) ||
    f === "manifest.json" ||
    f === "meet-ai.iconset"
  )
    rmSync(join(STAGE, f), { recursive: true, force: true });
}

const write = (rel, body) => {
  const p = join(BRAND, rel);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, body.replace(/\n\s*\n/g, "\n"));
  return p;
};

const HEAD = '<svg xmlns="http://www.w3.org/2000/svg"';

/* --------------------------------------------------------------------------
   THE SYMBOL — "m."
   One drawing (geometry.mjs mdot / mdotGrid) and one way to paint it. The m is
   a single <path>: its two arches share the middle leg, and one stroked path
   paints that overlap once rather than twice.
-------------------------------------------------------------------------- */
function glyph(parts, fg, accent) {
  const { stroke, paths, dot } = parts;
  return (
    `<path d="${paths.join(" ")}" fill="none" stroke="${fg}" stroke-width="${stroke}" stroke-linecap="round" stroke-linejoin="round"/>` +
    `<circle cx="${dot.cx}" cy="${dot.cy}" r="${dot.r}" fill="${accent}"/>`
  );
}

/* The free-standing symbol, off the tile: the m in ink or chalk, the dot in
   ember — the same accent the wordmark's `i` tittle carries, so symbol and
   wordmark share one colour idea. The artboard is square (logomarks get used
   as avatars) and centred on the symbol, with half a dot of air each side of
   the ink; the full clear space (README §4) is the user's to add. */
function logomark(spec, fg, accent) {
  const parts = mdot(spec);
  const { box } = parts;
  const side = box.w + 2 * spec.dotR;
  const x = box.x + box.w / 2 - side / 2;
  const y = box.y + box.h / 2 - side / 2;
  return `${HEAD} viewBox="${f(x)} ${f(y)} ${f(side)} ${f(side)}" width="100" height="100" fill="none" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  ${glyph(parts, fg, accent)}
</svg>`;
}

write("meet-ai-logomark-primary-ink.svg", logomark(MDOT, P.ink, P.ember));
write("meet-ai-logomark-primary-chalk.svg", logomark(MDOT, P.chalk, P.ember));
write("meet-ai-logomark-mono-black.svg", logomark(MDOT, "#000000", "#000000"));
write("meet-ai-logomark-mono-white.svg", logomark(MDOT, "#FFFFFF", "#FFFFFF"));
write("meet-ai-logomark-small-chalk.svg", logomark(MDOT_SMALL, P.chalk, P.ember));

/* Menu-bar template: pure black with alpha, drawn on the 16px pixel grid.
   macOS tints a template image itself, so it carries no brand colour. */
write(
  "meet-ai-menubar-template-black.svg",
  `${HEAD} viewBox="0 0 16 16" width="16" height="16" fill="none" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  ${glyph(mdotGrid(MENUBAR), "#000000", "#000000")}
</svg>`,
);

/* --------------------------------------------------------------------------
   WORDMARK + LOCKUP
-------------------------------------------------------------------------- */
const W = layoutWordmark();
const WPAD = W.stroke / 2;
const WTOP = 19 - 4.5; // top of the `i` tittle
const WBOT = 67.5 + WPAD; // baseline plus half a stroke
const WH = WBOT - WTOP;

function wordBody(fg, accent) {
  return (
    W.paths
      .map(
        (d) =>
          `<path d="${d}" fill="none" stroke="${fg}" stroke-width="${W.stroke}" stroke-linecap="round" stroke-linejoin="round"/>`,
      )
      .join("") +
    W.dots
      .map((d) => `<circle cx="${d.cx}" cy="${d.cy}" r="${d.r}" fill="${d.accent ? accent : fg}"/>`)
      .join("")
  );
}

const wordmark = (fg, accent) =>
  `${HEAD} viewBox="${-WPAD} ${WTOP} ${f(W.width + WPAD * 2)} ${f(WH)}" width="${f(
    W.width + WPAD * 2,
  )}" height="${f(WH)}" fill="none" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  ${wordBody(fg, accent)}
</svg>`;

write("meet-ai-wordmark-primary-ink.svg", wordmark(P.ink, P.ember));
write("meet-ai-wordmark-primary-chalk.svg", wordmark(P.chalk, P.ember));
write("meet-ai-wordmark-mono-black.svg", wordmark("#000000", "#000000"));
write("meet-ai-wordmark-mono-white.svg", wordmark("#FFFFFF", "#FFFFFF"));

/* Horizontal lockup: "m." then the wordmark. The symbol's ink box is set to
   exactly the wordmark's height (tittle top to baseline), which puts the two
   baselines level — the dot is a full stop, so it has to sit on the line the
   word sits on. That makes the symbol's x-height 1.3x the word's, which is
   the step a symbol needs over the text beside it; any larger and the pair
   read as two logos. The gap is two of the symbol's dot diameters: at one
   (the clear-space minimum, README §4) the pair read as a single word,
   "m.meet-ai". */
const MARK = mdot(MDOT);
const lockScale = WH / MARK.box.h;
const markW = MARK.box.w * lockScale;
const GAP = MDOT.dotR * 4 * lockScale;
const lockH = WH;
const lockW = markW + GAP + W.width + W.stroke;

function lockup(fg, accent) {
  const wy = -WTOP;
  const wx = markW + GAP + WPAD;
  return `${HEAD} viewBox="0 0 ${f(lockW)} ${f(lockH)}" width="${f(lockW)}" height="${f(
    lockH,
  )}" fill="none" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  <g transform="scale(${f(lockScale)}) translate(${f(-MARK.box.x)} ${f(-MARK.box.y)})">${glyph(MARK, fg, accent)}</g>
  <g transform="translate(${f(wx)} ${f(wy)})">${wordBody(fg, accent)}</g>
</svg>`;
}

write("meet-ai-logo-horizontal-ink.svg", lockup(P.ink, P.ember));
write("meet-ai-logo-horizontal-chalk.svg", lockup(P.chalk, P.ember));
write("meet-ai-logo-horizontal-mono-black.svg", lockup("#000000", "#000000"));
write("meet-ai-logo-horizontal-mono-white.svg", lockup("#FFFFFF", "#FFFFFF"));

/* --------------------------------------------------------------------------
   APP ICON
   1024 canvas, 824 body, superellipse corner (n=4.6). The 100px margin is the
   macOS grid's shadow room, not padding to fill.

   The symbol is drawn on the 1024 Icon Composer canvas, where the canvas is
   the tile. Here the tile is the 824 body, so the whole canvas drawing is
   mapped onto the body — the "m." takes the same share of the tile in the
   legacy files as in meet-ai.icon, by construction rather than by a second
   set of numbers.
-------------------------------------------------------------------------- */
const BODY_HALF = 412;
const SQ = squirclePath(512, 512, BODY_HALF);
const ONTO_BODY = `translate(${512 - BODY_HALF} ${512 - BODY_HALF}) scale(${f((BODY_HALF * 2) / 1024)})`;

// Dusk, in SVG's objectBoundingBox terms — the same numbers icon.json uses.
const dusk = (id) =>
  `<linearGradient id="${id}" x1="${TG.start.x}" y1="${TG.start.y}" x2="${TG.stop.x}" y2="${TG.stop.y}">
     <stop offset="0" stop-color="${P.tileTop}"/><stop offset="1" stop-color="${P.tileBottom}"/>
   </linearGradient>`;

function appIcon(spec, opts = {}) {
  const { flat = false, rim = !flat } = opts;
  /* The rim is the only depth drawn: a specular edge clipped to the body,
     light at the top and dark at the bottom. No drop shadow under the glyph,
     no glass — the system adds its own edge and shadow to the .icon, and a
     soft shadow under a glyph is the detail that dates first. */
  const defs = [
    flat ? "" : dusk("b"),
    rim
      ? `<linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
         <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/>
         <stop offset="0.42" stop-color="#FFFFFF" stop-opacity="0"/>
         <stop offset="1" stop-color="#000000" stop-opacity="0.18"/>
       </linearGradient>
       <clipPath id="body"><path d="${SQ}"/></clipPath>`
      : "",
  ].join("");

  return `${HEAD} viewBox="0 0 1024 1024" width="1024" height="1024" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  ${defs ? `<defs>${defs}</defs>` : ""}
  <path d="${SQ}" fill="${flat ? P.tileFlat : "url(#b)"}"/>
  ${rim ? `<g clip-path="url(#body)"><path d="${SQ}" fill="none" stroke="url(#rim)" stroke-width="5"/></g>` : ""}
  <g transform="${ONTO_BODY}">${glyph(mdot(spec), P.glyph, P.glyph)}</g>
</svg>`;
}

write("meet-ai-appicon-primary-fullcolor.svg", appIcon(MDOT));
write("meet-ai-appicon-small-fullcolor.svg", appIcon(MDOT_SMALL));
write("meet-ai-appicon-flat-fullcolor.svg", appIcon(MDOT, { flat: true }));

/* 16px app icon, drawn pixel by pixel rather than scaled down (see ICON16). */
{
  const g = ICON16;
  const sq16 = squirclePath(8, 8, 8 - g.bodyInset, 4.6, 64);
  const px = pixelRuns(g.rows)
    .map(
      (r) =>
        `<rect x="${r.x}" y="${r.y}" width="${r.w}" height="1" fill="${P.glyph}"${r.a < 1 ? ` fill-opacity="${r.a}"` : ""}/>`,
    )
    .join("");
  write(
    "meet-ai-appicon-16-fullcolor.svg",
    `${HEAD} viewBox="0 0 16 16" width="16" height="16" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  <defs>${dusk("b")}</defs>
  <path d="${sq16}" fill="url(#b)"/>
  <g shape-rendering="crispEdges">${px}</g>
</svg>`,
  );
}

/* Favicon: the small-grid symbol on the Dusk tile, no rim. The gradient stays:
   it is the part of the identity that survives at tab size. */
write("meet-ai-favicon.svg", appIcon(MDOT_SMALL, { rim: false }));

/* --------------------------------------------------------------------------
   BRAND TOKENS
   The contrast figures are computed here from the palette, not typed in, so
   they cannot go stale when a colour moves.
-------------------------------------------------------------------------- */
const lin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const lum = (hex) => {
  const [r, g, b] = [1, 3, 5].map((i) => lin(parseInt(hex.slice(i, i + 2), 16) / 255));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
const ratio = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
  return `${((x + 0.05) / (y + 0.05)).toFixed(1)}:1`;
};
const row = (label, a, b, verdict) => `     ${`${label} `.padEnd(26, ".")} ${ratio(a, b).padEnd(8)} ${verdict}`;

write(
  "brand-tokens.css",
  `/* =============================================================================
   meet-ai — brand tokens
   These are IDENTITY tokens: the app icon, the symbol, README and marketing
   surfaces. They are deliberately NOT wired into the product UI. The interface
   uses macOS system colours on purpose (design-system/meet-ai/tokens.css, the
   "macOS system colors" block). Do not repoint --accent at a brand colour.
   ============================================================================= */
:root {
  /* Dusk — the app-icon tile. Coral at the top, violet at the bottom. */
  --brand-tile-top:    ${P.tileTop};
  --brand-tile-bottom: ${P.tileBottom};
  --brand-tile-flat:   ${P.tileFlat};   /* one-colour stand-in for the gradient */
  --brand-glyph:       ${P.glyph};   /* "m." on the tile */

  /* Ink — the tile in Dark appearance, and one-colour dark. */
  --brand-ink:        ${P.ink};        /* flat / one-colour ink       */
  --brand-ink-top:    ${P.inkTop};     /* dark tile gradient, top     */
  --brand-ink-bottom: ${P.inkBottom};  /* dark tile gradient, bottom  */

  /* Chalk — the symbol and wordmark on dark. */
  --brand-chalk:      ${P.chalk};

  /* Ember — the accent off the tile: the free-standing symbol's dot and the
     wordmark's i tittle. */
  --brand-ember:       ${P.ember};
  --brand-ember-core:  ${P.emberCore};
  --brand-ember-rim:   ${P.emberRim};
  --brand-ember-ink:   ${P.emberInk};   /* ember as TEXT on white      */
}

/* Contrast (WCAG 2.1 relative luminance), computed by build.mjs:
${row("white on tile top", P.glyph, P.tileTop, "just clears 3:1 (graphics); rendered lower, README §3")}
${row("white on tile bottom", P.glyph, P.tileBottom, "AA large")}
${row("white on tile flat", P.glyph, P.tileFlat, "AA large")}
${row("coral dot on ink (Dark)", P.tileTop, P.ink, "AA")}
${row("chalk on ink", P.chalk, P.ink, "AAA")}
${row("ink on white", P.ink, "#FFFFFF", "AAA")}
${row("ember on ink", P.ember, P.ink, "AAA")}
${row("ember on white", P.ember, "#FFFFFF", "FAILS — never set ember as text on white")}
${row("ember-ink on white", P.emberInk, "#FFFFFF", "AA normal, AAA large")}
   Nothing in the identity depends on hue alone: "m." is a letter and a full
   stop, and reads the same in one colour. */
`,
);

/* --------------------------------------------------------------------------
   RENDER MANIFEST
   Each entry: an HTML shim (so Chrome lays the SVG out at exact pixel size
   with no document margin) plus the PNG to produce.
-------------------------------------------------------------------------- */
const targets = [];
function raster(name, svgRel, size) {
  // Inline rather than <img src>: a file:// page cannot reliably load a
  // file:// image, and an inline SVG also guarantees the exact pixel box.
  const svg = readFileSync(join(BRAND, svgRel), "utf8")
    .replace(/\swidth="[^"]*"/, ` width="${size}"`)
    .replace(/\sheight="[^"]*"/, ` height="${size}"`);
  const html = `<!doctype html><meta charset="utf-8"><style>html,body{margin:0;padding:0;background:transparent}svg{display:block}</style>${svg}`;
  const shim = join(STAGE, `${name}.html`);
  writeFileSync(shim, html);
  targets.push({ name, shim, png: join(STAGE, `${name}.png`), size });
}

/* App-icon rasters, three tiers:
     16px       -> the 16px-grid drawing (pixel aligned)
     20..64px   -> the small artwork (now the master's geometry, MDOT_SMALL)
     128px+     -> the primary artwork
   The crossover sits at 128, not 64: 64 is `icon_32x32@2x`, i.e. the 32pt
   design at 2x, so small art is correct there on Apple's own terms. */
const ICON_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 180, 256, 512, 1024];
for (const s of ICON_SIZES) {
  const art =
    s === 16
      ? "meet-ai-appicon-16-fullcolor.svg"
      : s <= 64
        ? "meet-ai-appicon-small-fullcolor.svg"
        : "meet-ai-appicon-primary-fullcolor.svg";
  raster(`icon-${s}`, art, s);
}

/* Menu-bar tray icon, 1x and 2x.
   Both come from the same 16px-grid drawing rather than from two separate
   masters: every coordinate in MENUBAR is a whole or half number, so doubling
   the viewport lands the 2x raster on the pixel grid too. render.sh copies the
   pair into src-tauri/icons/ under AppKit's `…Template` names — see the note
   there for why the suffix is load-bearing (TUR-23). */
raster("tray-16", "meet-ai-menubar-template-black.svg", 16);
raster("tray-32", "meet-ai-menubar-template-black.svg", 32);

/* Proof renders for review: the symbol alone. */
for (const s of [16, 32, 128, 512]) {
  raster(`mark-${s}`, s <= 32 ? "meet-ai-logomark-small-chalk.svg" : "meet-ai-logomark-primary-chalk.svg", s);
}

writeFileSync(
  join(STAGE, "manifest.json"),
  JSON.stringify({ brand: BRAND, repo: REPO, targets }, null, 2),
);

console.log(`SVG masters -> ${BRAND}`);
console.log(`render manifest -> ${join(STAGE, "manifest.json")} (${targets.length} rasters)`);
console.log(`lockup ${f(lockW)} x ${f(lockH)}, wordmark width ${W.width}`);
