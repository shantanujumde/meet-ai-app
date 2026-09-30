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
   ============================================================================= */
import { writeFileSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { PALETTE as P, BRACKETS, bracketPaths, f } from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(process.argv[2] ?? join(HERE, "..", "meet-ai.icon"));

/* The canvas IS the icon body. The system draws the squircle mask, the edge
   highlight and the drop shadow over the whole 1024 square, and adds the macOS
   grid margin itself — ictool's macOS export fills the full canvas edge to
   edge. So none of that is drawn here; baking in the 824 squircle from the
   legacy master would mask it twice and shrink the icon inside its own plate.

   The mark keeps the same share of the body it has in the legacy master
   (build.mjs appIcon: 0.58 of the 824 body). Here the body is 1024 wide, so
   the mark is 0.58 of that, and the two read at the same size. */
const CANVAS = 1024;
const MARK_SHARE = 0.58;

const g = BRACKETS;
const vx = g.stemL - g.stroke / 2;
const vy = g.top - g.stroke / 2;
const vw = g.stemR - g.stemL + g.stroke;
const vh = g.bottom - g.top + g.stroke;
const scale = (CANVAS * MARK_SHARE) / vw;
const tx = CANVAS / 2 - (vw * scale) / 2;
const ty = CANVAS / 2 - (vh * scale) / 2;
const place = `translate(${f(tx)} ${f(ty)}) scale(${f(scale)}) translate(${f(-vx)} ${f(-vy)})`;

/* Every layer is a full-canvas SVG with the art already in place, so no layer
   needs a position override in icon.json and the two cannot drift apart. */
const svg = (defs, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${CANVAS} ${CANVAS}" width="${CANVAS}" height="${CANVAS}">
${defs ? `  <defs>${defs}</defs>\n` : ""}  <g transform="${place}">
    ${body}
  </g>
</svg>
`;

const { left, right } = bracketPaths(g);
const stroke = `fill="none" stroke="${P.chalk}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
const brackets = svg("", `<path d="${left}" ${stroke}/><path d="${right}" ${stroke}/>`);

// The same vertical ramp as the legacy master's dot (see build.mjs).
const dot = svg(
  `<linearGradient id="dot" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0" stop-color="${P.emberCore}"/>
    <stop offset="0.5" stop-color="${P.ember}"/>
    <stop offset="1" stop-color="${P.emberRim}"/>
  </linearGradient>`,
  `<circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="url(#dot)"/>`,
);

// "#RRGGBB" -> "srgb:r,g,b,1.00000", the colour syntax icon.json uses.
const srgb = (hex) => {
  const c = [1, 3, 5].map((i) => (parseInt(hex.slice(i, i + 2), 16) / 255).toFixed(5));
  return `srgb:${c.join(",")},1.00000`;
};

/* The body is the document fill, not a layer: the fill is the part the
   system replaces with its own plate in the tinted and clear appearances,
   leaving the layers on top to carry the mark.

   Dark is pinned to the same ink. Left to itself the system replaces the fill
   with a neutral near-black (#1E1D1D, measured) — the icon is already a dark
   object, so that only drops the ink's blue cast. */
const body = {
  "linear-gradient": [srgb(P.inkTop), srgb(P.inkBottom)],
  orientation: { start: { x: 0.5, y: 0 }, stop: { x: 0.5, y: 1 } },
};

/* Glass is off on both layers. With the default Liquid Glass treatment,
   ictool renders a dark keyline around each bracket and dims the art:
   brackets F4F5F7 -> E6E7E9, dot FF8A3C -> E2884B at its centre. That is the
   "no drop shadow under the glyph" and "the mark is a solid object" rules in
   ../README.md §6, applied by the system instead of by us. Flat layers render
   the brand swatches exactly (F4F6F7 / FF893D) while the system still owns
   the mask, the edge highlight and the dark / tinted / clear appearances.

   The dot is also translucency-off: it is the record light, and a light that
   lets the body show through reads as off. With glass off this changes
   nothing today; it keeps the dot opaque if glass is ever turned back on
   (with glass on, it took the dot's centre from E2884B back to F08F4E).

   Tinted and clear are the system's one-colour renderings, so the dot takes
   the one-colour rule every -mono- file here already follows: same colour as
   the brackets. Left as ember, the system maps it by luminance and it sinks
   into the body — #493480 against #5E41AF brackets in TintedDark, nearly gone
   at 32px. The "tinted" appearance covers the clear renditions too (checked:
   the ClearDark dot went #ADAEAE -> #E8E8E9). The dot stays readable there
   because it is the only filled shape, not because of its hue (§3).

   Two groups, not one, so that turning glass back on later gives the dot its
   own depth above the brackets rather than fusing them into one slab. The
   format allows four.

   Not carried over from the legacy master: the specular rim (the system draws
   its own edge highlight) and the 13% ember halo. The layers are exactly the
   mark's two solid shapes and nothing soft. */
const oneColour = { solid: srgb(P.chalk) };
const icon = {
  "fill-specializations": [{ value: body }, { appearance: "dark", value: body }],
  groups: [
    {
      layers: [
        {
          "image-name": "dot.svg",
          name: "dot",
          glass: false,
          "fill-specializations": [{ appearance: "tinted", value: oneColour }],
        },
      ],
      translucency: { enabled: false, value: 0.5 },
    },
    {
      layers: [{ "image-name": "brackets.svg", name: "brackets", glass: false }],
    },
  ],
  "supported-platforms": { squares: ["macOS"] },
};

// Assets/ is written from icon.json's own layer list, so the two cannot
// disagree. That matters because ictool renders a layer whose image-name points
// nowhere as if the layer were not there — no error, just a missing dot.
const artwork = { "dot.svg": dot, "brackets.svg": brackets };
const names = icon.groups.flatMap((grp) => grp.layers).map((l) => l["image-name"]);

rmSync(OUT, { recursive: true, force: true });
mkdirSync(join(OUT, "Assets"), { recursive: true });
for (const name of names) {
  if (!artwork[name]) throw new Error(`icon.json names a layer with no artwork: ${name}`);
  writeFileSync(join(OUT, "Assets", name), artwork[name]);
}
writeFileSync(join(OUT, "icon.json"), `${JSON.stringify(icon, null, 2)}\n`);
console.log(`   ${OUT.split("/").pop()}  icon.json + ${names.map((n) => `Assets/${n}`).join(", ")}`);
