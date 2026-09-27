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
  BRACKETS,
  BRACKETS_SMALL,
  MENUBAR,
  ICON16,
  bracketPaths,
  squirclePath,
  layoutWordmark,
  f,
} from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BRAND = resolve(HERE, "..");
const REPO = resolve(HERE, "..", "..", "..", "..");
const STAGE = join(HERE, ".render");
// Prune only what this script owns. `.render/` is shared: proof.mjs and
// proximity.mjs stage their HTML here too, so that relative <img> links to the
// rasters resolve under file://. Wiping the directory wholesale used to delete
// those pages out from under their render scripts — and because Chrome answers
// a missing file:// URL with an error page rather than a non-zero exit, the
// result was a screenshot of the failure, committed as a proof (TUR-12).
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
   MARK
-------------------------------------------------------------------------- */
function mark(g, fg, accent) {
  const { left, right } = bracketPaths(g);
  const s = `fill="none" stroke="${fg}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  return (
    `<path d="${left}" ${s}/><path d="${right}" ${s}/>` +
    `<circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${accent}"/>`
  );
}

const logomark = (g, fg, accent, title) =>
  `${HEAD} viewBox="0 0 100 100" width="100" height="100" fill="none" role="img" aria-label="${title}">
  <title>${title}</title>
  ${mark(g, fg, accent)}
</svg>`;

write("meet-ai-logomark-primary-ink.svg", logomark(BRACKETS, P.ink, P.ember, "meet-ai"));
write("meet-ai-logomark-primary-chalk.svg", logomark(BRACKETS, P.chalk, P.ember, "meet-ai"));
write("meet-ai-logomark-mono-black.svg", logomark(BRACKETS, "#000000", "#000000", "meet-ai"));
write("meet-ai-logomark-mono-white.svg", logomark(BRACKETS, "#FFFFFF", "#FFFFFF", "meet-ai"));
write("meet-ai-logomark-small-chalk.svg", logomark(BRACKETS_SMALL, P.chalk, P.ember, "meet-ai"));

/* Menu-bar template: pure black with alpha, drawn on the 16px pixel grid.
   macOS tints a template image itself, so it carries no brand colour. */
{
  const g = MENUBAR;
  const { left, right } = bracketPaths(g);
  const s = `fill="none" stroke="#000000" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  write(
    "meet-ai-menubar-template-black.svg",
    `${HEAD} viewBox="0 0 16 16" width="16" height="16" fill="none" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  <path d="${left}" ${s}/><path d="${right}" ${s}/>
  <circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="#000000"/>
</svg>`,
  );
}

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

/* Horizontal lockup. The mark is set to 1.24x the wordmark's tittle-to-baseline
   height. Matching the boxes exactly makes the mark look short: the wordmark's
   height is mostly one tall `t` and a floating tittle, while the mark is solid
   from edge to edge, so equal boxes are not equal presence. 1.16 was still
   visibly short against the word in the README render; 1.24 sits level. */
const MARK_VIS = { x0: 16 - BRACKETS.stroke / 2, y0: 20 - BRACKETS.stroke / 2 };
const markVisW = BRACKETS.stemR - BRACKETS.stemL + BRACKETS.stroke;
const markVisH = BRACKETS.bottom - BRACKETS.top + BRACKETS.stroke;
const lockScale = (WH * 1.24) / markVisH;
const GAP = WH * 0.42; // clear space between mark and wordmark
const lockH = markVisH * lockScale;
const lockW = markVisW * lockScale + GAP + W.width + W.stroke;

function lockup(fg, accent) {
  const my = (lockH - markVisH * lockScale) / 2;
  const wy = (lockH - WH) / 2 - WTOP;
  const wx = markVisW * lockScale + GAP + WPAD;
  return `${HEAD} viewBox="0 0 ${f(lockW)} ${f(lockH)}" width="${f(lockW)}" height="${f(
    lockH,
  )}" fill="none" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  <g transform="translate(0 ${f(my)}) scale(${f(lockScale)}) translate(${f(-MARK_VIS.x0)} ${f(
    -MARK_VIS.y0,
  )})">${mark(BRACKETS, fg, accent)}</g>
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
-------------------------------------------------------------------------- */
const BODY_HALF = 412;
const SQ = squirclePath(512, 512, BODY_HALF);

function appIcon(g, markScaleFactor, opts = {}) {
  const { flat = false } = opts;
  const scale = (824 * markScaleFactor) / markVisW;
  const vx = g.stemL - g.stroke / 2;
  const vy = g.top - g.stroke / 2;
  const vw = g.stemR - g.stemL + g.stroke;
  const vh = g.bottom - g.top + g.stroke;
  const tx = 512 - (vw * scale) / 2;
  const ty = 512 - (vh * scale) / 2;

  const defs = flat
    ? `<linearGradient id="b" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${P.ink}"/><stop offset="1" stop-color="${P.ink}"/></linearGradient>`
    : `<linearGradient id="b" x1="0" y1="0" x2="0" y2="1">
         <stop offset="0" stop-color="${P.inkTop}"/><stop offset="1" stop-color="${P.inkBottom}"/>
       </linearGradient>
       <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1">
         <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.16"/>
         <stop offset="0.42" stop-color="#FFFFFF" stop-opacity="0"/>
         <stop offset="1" stop-color="#000000" stop-opacity="0.22"/>
       </linearGradient>
       <!-- Vertical, not radial. An offset radial highlight turned the dot into
            a shaded 3D ball, which is the most dateable thing an icon can do.
            A top-to-bottom ramp matches the body's own gradient, so the dot
            reads as a lit surface in the same light rather than an orb. -->
       <linearGradient id="dot" x1="0" y1="0" x2="0" y2="1">
         <stop offset="0" stop-color="${P.emberCore}"/>
         <stop offset="0.5" stop-color="${P.ember}"/>
         <stop offset="1" stop-color="${P.emberRim}"/>
       </linearGradient>
       <radialGradient id="halo" cx="0.5" cy="0.5" r="0.5">
         <stop offset="0" stop-color="${P.ember}" stop-opacity="0.13"/>
         <stop offset="1" stop-color="${P.ember}" stop-opacity="0"/>
       </radialGradient>
       <clipPath id="body"><path d="${SQ}"/></clipPath>`;

  /* No drop-shadow filter on the mark. A feDropShadow inside the ~9x scaled
     group rendered as opaque black boxes behind the stems, and the honest fix
     is not a bigger filter region — the icon does not need it. Depth comes
     from three real sources instead: the body gradient, the specular rim on
     the squircle, and the dot's own radial. That also keeps the artwork
     timeless; a soft shadow under a glyph is the detail that dates first. */
  const dotFill = flat ? P.ember : "url(#dot)";
  const { left, right } = bracketPaths(g);
  const strokeAttrs = `fill="none" stroke="${P.chalk}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  const halo = flat
    ? ""
    : `<circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${f(g.dot.r * 2.3)}" fill="url(#halo)"/>`;

  return `${HEAD} viewBox="0 0 1024 1024" width="1024" height="1024" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  <defs>${defs}</defs>
  <path d="${SQ}" fill="url(#b)"/>
  ${flat ? "" : `<g clip-path="url(#body)"><path d="${SQ}" fill="none" stroke="url(#rim)" stroke-width="5"/></g>`}
  <g transform="translate(${f(tx)} ${f(ty)}) scale(${f(scale)}) translate(${f(-vx)} ${f(-vy)})">
    ${halo}
    <path d="${left}" ${strokeAttrs}/><path d="${right}" ${strokeAttrs}/>
    <circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${dotFill}"/>
  </g>
</svg>`;
}

/* 0.66 of the body put the bracket stems almost on the squircle wall. 0.58
   gives the mark a margin roughly equal to its own stroke, which is what makes
   it sit calmly rather than press outward. The small variant stays larger
   (0.68) because below 48px the margin costs more than it buys. */
write("meet-ai-appicon-primary-fullcolor.svg", appIcon(BRACKETS, 0.58));
write("meet-ai-appicon-small-fullcolor.svg", appIcon(BRACKETS_SMALL, 0.68));
write("meet-ai-appicon-flat-fullcolor.svg", appIcon(BRACKETS, 0.58, { flat: true }));

/* 16px app icon, drawn on the pixel grid rather than scaled down. */
{
  const g = ICON16;
  const { left, right } = bracketPaths(g);
  const sq16 = squirclePath(8, 8, 8 - g.bodyInset, 4.6, 64);
  const s = `fill="none" stroke="${P.chalk}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  write(
    "meet-ai-appicon-16-fullcolor.svg",
    `${HEAD} viewBox="0 0 16 16" width="16" height="16" role="img" aria-label="meet-ai">
  <title>meet-ai</title>
  <path d="${sq16}" fill="${P.ink}"/>
  <path d="${left}" ${s}/><path d="${right}" ${s}/>
  <circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${P.ember}"/>
</svg>`,
  );
}

/* Favicon: the small-grid mark on the icon body, no depth. A favicon is never
   larger than 32px in practice and the gradient just muddies it. */
write("meet-ai-favicon.svg", appIcon(BRACKETS_SMALL, 0.68, { flat: true }));

/* --------------------------------------------------------------------------
   BRAND TOKENS
-------------------------------------------------------------------------- */
write(
  "brand-tokens.css",
  `/* =============================================================================
   meet-ai — brand tokens
   These are IDENTITY tokens: the app icon, the mark, README and marketing
   surfaces. They are deliberately NOT wired into the product UI. The interface
   uses macOS system colours on purpose (design-system/meet-ai/tokens.css, the
   "macOS system colors" block). Do not repoint --accent at --brand-ember.
   ============================================================================= */
:root {
  /* Ink — the icon body. A dark, quiet object in the Dock. */
  --brand-ink:        ${P.ink};        /* flat / one-colour ink       */
  --brand-ink-top:    ${P.inkTop};     /* icon gradient, top          */
  --brand-ink-bottom: ${P.inkBottom};  /* icon gradient, bottom       */

  /* Chalk — the mark on ink. */
  --brand-chalk:      ${P.chalk};

  /* Ember — the record light. The one warm thing in the system. */
  --brand-ember:       ${P.ember};      /* primary brand colour        */
  --brand-ember-core:  ${P.emberCore};  /* highlight inside the dot    */
  --brand-ember-rim:   ${P.emberRim};   /* shadow side of the dot      */
  --brand-ember-ink:   ${P.emberInk};   /* ember as TEXT on white      */
}

/* Contrast, measured (WCAG 2.1 relative luminance):
     chalk on ink ......... 16.2:1   AAA
     ink on white ......... 17.7:1   AAA
     ember on ink ..........7.6:1    AAA
     ember on white ........2.1:1    FAILS — never set ember as text on white
     ember-ink on white ....6.1:1    AAA large / AA normal
   Nothing in the identity depends on hue alone: the record dot is also the
   only filled shape in the mark, so it survives greyscale and colour blindness. */
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
     20..64px   -> the small-grid artwork
     128px+     -> the primary artwork
   The crossover sits at 128, not 64: at 64 the primary mark's stroke was
   visibly lighter than the 48px tile beside it, and a weight jump between
   adjacent icon sizes is the kind of thing you only see once you line them up.
   64 is also `icon_32x32@2x`, i.e. the 32pt design at 2x — small art is
   correct there on Apple's own terms. */
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
   masters: every coordinate in MENUBAR is a whole number, so doubling the
   viewport lands the 2x raster on the pixel grid too. render.sh copies the
   pair into src-tauri/icons/ under AppKit's `…Template` names — see the note
   there for why the suffix is load-bearing (TUR-23). */
raster("tray-16", "meet-ai-menubar-template-black.svg", 16);
raster("tray-32", "meet-ai-menubar-template-black.svg", 32);

/* Proof renders for review: mark alone, on light and on dark. */
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
