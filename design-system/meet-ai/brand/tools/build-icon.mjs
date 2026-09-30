/* =============================================================================
   meet-ai brand — Icon Composer document (TUR-87)
   Writes ../meet-ai.icon, the layered source that actool compiles into
   Assets.car for macOS 26. Generated from geometry.mjs like every other master
   here, so nothing inside meet-ai.icon/ is hand-edited.

     node build-icon.mjs [out.icon]   # default: ../meet-ai.icon

   render.sh calls this. It needs node and nothing else: Xcode is only needed
   later, to compile the document into Assets.car.

   A .icon is a folder: icon.json plus the layer art under Assets/. The keys
   below were taken from what Icon Composer itself reads (the strings in
   IconComposerFoundation) and checked by rendering every macOS appearance
   with Icon Composer's own ictool — not written from memory.

   The folder must stay `meet-ai.icon`: actool names the icon after its stem,
   and CFBundleIconName in src-tauri/Info.plist has to match it.
   ============================================================================= */
import { writeFileSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PALETTE as P, TILE_GRADIENT, GLYPH_GRADIENT, MDOT, mdot } from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(process.argv[2] ?? join(HERE, "..", "meet-ai.icon"));

/* The canvas IS the tile. The system draws the squircle mask, the edge
   highlight and the drop shadow over the whole 1024 square, and adds the macOS
   grid margin itself — ictool's macOS export fills the full canvas edge to
   edge. So none of that is drawn here; baking in the 824 squircle from the
   legacy master would mask it twice and shrink the icon inside its own plate.
   MDOT is specified on exactly this canvas, so the layers are drawn at 1:1. */
const CANVAS = 1024;
const parts = mdot(MDOT);

/* Every layer is a full-canvas SVG with the art already in place, so no layer
   needs a position override in icon.json and the two cannot drift apart.
   Layer art is drawn white: a layer's fill-specializations recolour it per
   appearance, and white is what the Default appearance wants anyway. */
const svg = (body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${CANVAS} ${CANVAS}" width="${CANVAS}" height="${CANVAS}">
  ${body}
</svg>
`;
const m = svg(
  `<path d="${parts.paths.join(" ")}" fill="none" stroke="${P.glyph}" stroke-width="${parts.stroke}" stroke-linecap="round" stroke-linejoin="round"/>`,
);
const dot = svg(`<circle cx="${parts.dot.cx}" cy="${parts.dot.cy}" r="${parts.dot.r}" fill="${P.glyph}"/>`);

// "#RRGGBB" -> "srgb:r,g,b,1.00000", the colour syntax icon.json uses.
const srgb = (hex) => {
  const c = [1, 3, 5].map((i) => (parseInt(hex.slice(i, i + 2), 16) / 255).toFixed(5));
  return `srgb:${c.join(",")},1.00000`;
};
const gradient = (top, bottom, orientation) => ({
  "linear-gradient": [srgb(top), srgb(bottom)],
  orientation,
});

/* The tile is the document fill, not a layer: the fill is the part the system
   replaces with its own plate in the tinted and clear appearances, leaving
   the layers on top to carry the mark.

   Default: Dusk, top to bottom (TILE_GRADIENT — the same numbers the legacy
   SVGs use). Dark: the brand's ink, top to bottom. Left to itself the system
   would keep Dusk in Dark, a bright tile in a dark Dock; ink is the brand's
   own dark object, and the colour moves into the glyph instead (below). */
const dusk = gradient(P.tileTop, P.tileBottom, TILE_GRADIENT);
const duskGlyph = gradient(P.tileTop, P.tileBottom, GLYPH_GRADIENT);
const ink = gradient(P.inkTop, P.inkBottom, { start: { x: 0.5, y: 0 }, stop: { x: 0.5, y: 1 } });

/* Glass is off on both layers, as the approved prototype had it. With Liquid
   Glass on, ictool outlined the previous mark's glyph in a dark keyline and
   dimmed it (TUR-87) — a drop shadow and an effect on a solid object, which
   README §6 rules out, applied by the system. With it off, the layers render
   flat white while the system still owns the mask, the edge highlight and the
   dark / tinted / clear appearances.

   Dark: the m wears Dusk, spread over its own bounds (a layer gradient maps
   to the layer's ink, not the canvas) on GLYPH_GRADIENT's diagonal, so it
   runs coral at the top-left shoulder to violet at the right foot.
   The dot does NOT get its own copy of the gradient. The approved prototype
   did that, and a 124-unit dot holding the whole coral-to-violet ramp read as
   a second, smaller icon. It takes coral instead: the record light, lit, and
   the one hue that stays distinct from the violet foot beside it, which keeps
   the full stop visibly separate from the m at 16px and 32px (checked on
   ictool's renders).

   Tinted and clear: plain white on both layers, pinned explicitly so the Dark
   gradient cannot carry into TintedDark / ClearDark. The system maps a
   one-colour rendering by luminance, so white is what gives the glyph the
   full tint. The "tinted" appearance covers the clear renditions too
   (checked in TUR-87, and again on this document's ClearDark render).

   Two groups, not one, so that turning glass back on later gives the dot its
   own depth above the m rather than fusing them into one slab. The dot also
   keeps translucency off: with glass on, a record light that lets the tile
   show through reads as off. With glass off this changes nothing. */
const white = { solid: srgb(P.glyph) };
const icon = {
  "fill-specializations": [{ value: dusk }, { appearance: "dark", value: ink }],
  groups: [
    {
      layers: [
        {
          "image-name": "dot.svg",
          name: "dot",
          glass: false,
          "fill-specializations": [
            { appearance: "dark", value: { solid: srgb(P.tileTop) } },
            { appearance: "tinted", value: white },
          ],
        },
      ],
      translucency: { enabled: false, value: 0.5 },
    },
    {
      layers: [
        {
          "image-name": "m.svg",
          name: "m",
          glass: false,
          "fill-specializations": [
            { appearance: "dark", value: duskGlyph },
            { appearance: "tinted", value: white },
          ],
        },
      ],
    },
  ],
  "supported-platforms": { squares: ["macOS"] },
};

// Assets/ is written from icon.json's own layer list, so the two cannot
// disagree. That matters because ictool renders a layer whose image-name points
// nowhere as if the layer were not there — no error, just a missing dot.
const artwork = { "dot.svg": dot, "m.svg": m };
const names = icon.groups.flatMap((grp) => grp.layers).map((l) => l["image-name"]);

rmSync(OUT, { recursive: true, force: true });
mkdirSync(join(OUT, "Assets"), { recursive: true });
for (const name of names) {
  if (!artwork[name]) throw new Error(`icon.json names a layer with no artwork: ${name}`);
  writeFileSync(join(OUT, "Assets", name), artwork[name]);
}
writeFileSync(join(OUT, "icon.json"), `${JSON.stringify(icon, null, 2)}\n`);
console.log(`   ${OUT.split("/").pop()}  icon.json + ${names.map((n) => `Assets/${n}`).join(", ")}`);
