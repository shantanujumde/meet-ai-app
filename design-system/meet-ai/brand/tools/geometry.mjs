/* =============================================================================
   meet-ai brand — geometry source of truth
   Every SVG in ../ is generated from this file. Edit here, run `build.mjs`,
   never hand-edit an output SVG.

   Two coordinate systems, one per drawing:
     - the wordmark is drawn on a 100-unit grid (baseline 67.5);
     - the symbol, "m.", is drawn on the 1024 Icon Composer canvas, where the
       canvas IS the app-icon tile. That is the drawing the board approved,
       and every other placement (legacy squircle, lockup, logomark) is a
       transform of it, so the proportions cannot drift between files.
   The two small drawings (the 16px app icon and the menu-bar template) are
   separate, on the 16px pixel grid.
   All stroke geometry is expressed on the *centreline*, so the visual
   bounding box is the centreline box grown by half a stroke.
   ============================================================================= */

export const PALETTE = {
  /* Dusk — the app-icon tile (2026-09-30). Coral at the top, violet at the
     bottom. The top stop was deepened from the approved #FF7A45 to #F46A3A to
     lift white-on-coral from 2.6:1 to 3.0:1; see ../README.md §3. */
  tileTop: "#F46A3A",
  tileBottom: "#9B3CF2",
  /* One colour for the tile where a gradient cannot be reproduced (the flat
     app-icon file, print). The gradient's sRGB midpoint. */
  tileFlat: "#C85396",
  glyph: "#FFFFFF", // "m." on the tile
  inkTop: "#24262E",
  inkBottom: "#121317",
  ink: "#16181D", // flat one-colour ink; the tile in Dark appearance
  chalk: "#F4F5F7", // the symbol and wordmark on dark
  emberCore: "#FFC152",
  ember: "#FF8A3C", // the accent: the dot of the free-standing symbol, the `i` tittle
  emberRim: "#E85F21",
  emberInk: "#A8410D", // accessible-on-white text variant
};

/* Dusk runs straight down, top edge to bottom edge. Written in Icon Composer's
   terms (unit square, y down), which are also SVG's objectBoundingBox terms,
   so the .icon and the legacy SVGs use the same numbers.
   The approved prototype asked for a slight diagonal (x 0.25 -> 0.75), but
   ictool renders a tile fill vertically whatever x says — measured on its
   render, the left and right edges are identical at every height. So the look
   that was approved is vertical, and saying so here keeps Chrome's legacy
   rasters (which do honour x) from drifting to a diagonal nobody signed off.
   A layer fill does honour x, so the glyph's own Dusk in Dark appearance keeps
   the prototype's diagonal: coral at the left shoulder, violet at the right
   foot (measured: left foot #CF69B5, right foot #AC5CF5). */
export const TILE_GRADIENT = { start: { x: 0.5, y: 0 }, stop: { x: 0.5, y: 1 } };
export const GLYPH_GRADIENT = { start: { x: 0.25, y: 0 }, stop: { x: 0.75, y: 1 } };

/* -----------------------------------------------------------------------------
   Superellipse squircle — Apple's app-icon corner is a continuous-curvature
   superellipse, not a rounded rect. A plain `rx` reads subtly wrong beside
   native icons: the corner arc starts too late and ends too abruptly.
   |x/a|^n + |y/a|^n = 1, sampled and emitted as a polyline-free smooth path.
-------------------------------------------------------------------------- */
export function squirclePath(cx, cy, half, n = 4.6, steps = 128) {
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const ct = Math.cos(t);
    const st = Math.sin(t);
    const x = cx + half * Math.sign(ct) * Math.abs(ct) ** (2 / n);
    const y = cy + half * Math.sign(st) * Math.abs(st) ** (2 / n);
    pts.push([x, y]);
  }
  // Catmull-Rom through the samples -> cubic beziers. Smooth, closed.
  let d = `M${f(pts[0][0])},${f(pts[0][1])}`;
  for (let i = 0; i < pts.length; i++) {
    const p0 = pts[(i - 1 + pts.length) % pts.length];
    const p1 = pts[i];
    const p2 = pts[(i + 1) % pts.length];
    const p3 = pts[(i + 2) % pts.length];
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${f(c1[0])},${f(c1[1])} ${f(c2[0])},${f(c2[1])} ${f(p2[0])},${f(p2[1])}`;
  }
  return `${d}Z`;
}

const f = (v) => Math.round(v * 1000) / 1000;

/* -----------------------------------------------------------------------------
   WORDMARK — "meet-ai"
   Drawn, not set. No licensed typeface is involved. One monoline
   construction: one stroke weight, round terminals, circular bowls. The `i`
   tittle is the record dot. The symbol, "m." (below), is this wordmark's own
   m set heavy, which is what ties the two together.

   Baseline 67.5 (centreline), x-height top 32.5 (centreline), ascender 18.5.
   Letter stroke is 9 on this 100-unit grid.
-------------------------------------------------------------------------- */
export const WORD = {
  stroke: 9,
  baseline: 67.5,
  xtop: 32.5,
  asc: 18.5,
  bowl: 17.5, // centreline radius of e / a
  arch: 13, // m shoulder radius
  tittleR: 4.5,
  tittleY: 19, // half a stroke of air above the stem, not a floating speck
};

function arcAround(cx, cy, r, a0, a1, large, sweep) {
  const p = (a) => [cx + r * Math.cos(a), cy + r * Math.sin(a)];
  const [x0, y0] = p(a0);
  const [x1, y1] = p(a1);
  return {
    start: `M${f(x0)},${f(y0)}`,
    arc: `A${r},${r} 0 ${large} ${sweep} ${f(x1)},${f(y1)}`,
  };
}

/* Each glyph returns { paths: [d...], dots: [{cx,cy,r,accent}], advance }.
   `advance` is the centreline advance; side bearings are added by layout. */
export function glyphs(w = WORD) {
  const { baseline: B, xtop: XT, asc: A, bowl: R, arch: AR, stroke: S } = w;
  const mid = (B + XT) / 2; // 50

  const m = () => {
    const x = 0;
    const spring = XT + AR;
    return {
      paths: [
        `M${x},${B} V${f(spring)} A${AR},${AR} 0 0 1 ${f(x + AR * 2)},${f(spring)} V${B}`,
        `M${f(x + AR * 2)},${f(spring)} A${AR},${AR} 0 0 1 ${f(x + AR * 4)},${f(spring)} V${B}`,
      ],
      dots: [],
      advance: AR * 4,
    };
  };

  const e = () => {
    const cx = R;
    /* The bar starts at the right edge of the bowl and the bowl terminal sits
       60 degrees below it. At the textbook -35 degrees the aperture measured
       1.7 units of actual white against a 9-unit stroke, and the glyph read as
       a barred circle, not an `e`. -60 gives ~9 units of white. */
    const th = (60 * Math.PI) / 180; // SVG y grows downward, so +60 is below
    const ex = cx + R * Math.cos(th);
    const ey = mid + R * Math.sin(th);
    // From 3 o'clock, anticlockwise on screen (sweep 0) the long way round.
    return {
      paths: [
        `M0,${mid} H${f(R * 2)}`,
        `M${f(cx + R)},${mid} A${R},${R} 0 1 0 ${f(ex)},${f(ey)}`,
      ],
      dots: [],
      advance: R * 2,
    };
  };

  const t = () => {
    /* Asymmetric crossbar and a tail. A centred crossbar on a straight stem
       reads as a plus sign at small sizes; the tail is what fixes it. */
    const stem = 8;
    const tailR = 8;
    return {
      paths: [
        `M${stem},${A} V${f(B - tailR)} A${tailR},${tailR} 0 0 0 ${f(stem + tailR)},${B}`,
        `M0,${XT} H18`,
      ],
      dots: [],
      advance: 18,
    };
  };

  const hyphen = () => ({ paths: [`M0,${mid} H20`], dots: [], advance: 20 });

  const a = () => {
    /* Single-storey geometric `a`: a closed bowl with the stem tangent to it
       on the right. Drawn as two semicircles — a single 360-degree arc has
       coincident endpoints and SVG collapses it to nothing. */
    const cx = R;
    return {
      paths: [
        `M${cx},${f(mid - R)} A${R},${R} 0 1 1 ${cx},${f(mid + R)} A${R},${R} 0 1 1 ${cx},${f(mid - R)}Z`,
        `M${f(R * 2)},${XT} V${B}`,
      ],
      dots: [],
      advance: R * 2,
    };
  };

  const i = () => ({
    paths: [`M0,${XT} V${B}`],
    dots: [{ cx: 0, cy: w.tittleY, r: w.tittleR, accent: true }],
    advance: 0,
  });

  return { m, e, t, hyphen, a, i, stroke: S };
}

/* Hand-kerned. Round terminals need more air than a normal sans, and the
   pairs below are tuned by eye, not by a single tracking value:
   `ee` opens up (two circles), `t-` tightens (the crossbar already reads as
   space), `ai` tightens (the `a` stem and the `i` stem must not read as `u`
   but must not drift apart either). */
export const KERN = { "me": 15, "ee": 17, "et": 14, "t-": 9, "-a": 11, "ai": 16 };

export function layoutWordmark() {
  const G = glyphs();
  const order = [
    ["m", G.m()],
    ["e", G.e()],
    ["e", G.e()],
    ["t", G.t()],
    ["-", G.hyphen()],
    ["a", G.a()],
    ["i", G.i()],
  ];
  const paths = [];
  const dots = [];
  let x = 0;
  for (let k = 0; k < order.length; k++) {
    const [, g] = order[k];
    for (const d of g.paths) paths.push(translate(d, x));
    for (const dt of g.dots) dots.push({ ...dt, cx: dt.cx + x });
    x += g.advance;
    if (k < order.length - 1) {
      const pair = order[k][0] + order[k + 1][0];
      x += KERN[pair] ?? 15;
    }
  }
  return { paths, dots, width: x, stroke: G.stroke };
}

/* Translate an absolute-command path along x. Our glyph paths only use
   M/H/V/A/Z with absolute coordinates, so this is a safe token rewrite. */
export function translate(d, dx) {
  return d.replace(/([MHVA])([^MHVAZ]*)/g, (_, cmd, args) => {
    const nums = args.trim().split(/[\s,]+/).filter(Boolean).map(Number);
    if (cmd === "M") return `M${f(nums[0] + dx)},${f(nums[1])}`;
    if (cmd === "H") return `H${f(nums[0] + dx)}`;
    if (cmd === "V") return `V${f(nums[0])}`;
    if (cmd === "A")
      return `A${nums[0]},${nums[1]} ${nums[2]} ${nums[3]} ${nums[4]} ${f(nums[5] + dx)},${f(nums[6])}`;
    return _;
  });
}

export { f };

/* -----------------------------------------------------------------------------
   THE SYMBOL — "m."   (adopted 2026-09-30, replacing `[ · ]`)
   The wordmark's own m — monoline, round terminals, circular arches — set
   heavy, with the record dot after it as a full stop. White on the Dusk tile.

   Built from glyphs().m() rather than redrawn, so the symbol and the first
   letter of the wordmark are the same construction at two weights. Only the
   weight changes: the wordmark m is stroke 9 on arch 13 (0.69), the symbol is
   stroke 100 on arch 104 (0.96), heavy enough that at 16px the three legs are
   still three legs and the two counters are still open.

   Numbers are on the 1024 Icon Composer canvas, which is the tile edge to
   edge, and are the approved prototype's
   (target/icon-lab/concepts/colour/build-colour.mjs, concept B):
     - x-height 356..668 on the centreline, so the ink runs 306..718 and the
       mark is centred on the canvas's 512 vertically;
     - the dot sits on the baseline — its bottom is level with the bottom of
       the legs' round caps;
     - the whole "m." is centred horizontally as a unit.
   One change from the prototype: the gap between the last leg and the dot,
   44 -> 60. A .icon has one drawing for every size, and ictool's 16px @1x
   render of the 44 gap fused the dot onto the last leg in Default, Tinted and
   Clear — the "m." read as an m with a foot. Tested 44 / 60 / 76 / 92: 60 is
   the smallest that keeps a light column between them at 16px, and at Dock
   sizes it still reads as a full stop rather than a separate dot. Width 700.
   The dot is the full stop, not an `i` tittle. It was moved off the tittle
   position deliberately: a white lower-case "mi" with a dot over the i is
   Xiaomi's old mark. */
export const MDOT = {
  stroke: 100,
  arch: 104, // shoulder radius, centreline
  xtop: 356, // x-height, centreline
  baseline: 668, // centreline
  gap: 60, // white between the last leg and the dot
  dotR: 62,
  cx: 512, // horizontal centre of the whole "m."
};

/* The same symbol for 20..64px rasters and the favicon: identical m, the dot
   pushed 16 units further out. At 20px the master's 60-unit gap is under a
   pixel and the dot starts to fuse with the last leg; 76 keeps a whole pixel of
   white down to 20px. Dock sizes use MDOT. */
export const MDOT_SMALL = { ...MDOT, gap: 76 };

/* Resolve a symbol spec to drawable parts: the m's centreline paths, the dot,
   and the visual (ink) box. Every consumer draws from this — build.mjs's SVG
   masters and lockup, build-icon.mjs's .icon layers, and (through
   mdotGrid) the menu-bar template — so there is one construction. */
export function mdot(g = MDOT) {
  const { stroke: S, arch: AR, xtop, baseline: B, gap, dotR: r } = g;
  const m = glyphs({ ...WORD, stroke: S, arch: AR, xtop, baseline: B }).m();
  const width = S + 4 * AR + gap + 2 * r; // ink, left edge of leg 1 .. right of dot
  const x0 = g.cx - width / 2 + S / 2; // centreline of the first leg
  return {
    stroke: S,
    paths: m.paths.map((d) => translate(d, x0)),
    dot: { cx: f(x0 + 4 * AR + S / 2 + gap + r), cy: f(B + S / 2 - r), r },
    box: { x: f(x0 - S / 2), y: f(xtop - S / 2), w: f(width), h: f(B - xtop + S) },
  };
}

/* App icon drawn pixel by pixel on the 16px grid (favicon, .ico). Scaling the
   1024 master down lands every leg on a fractional pixel and turns it grey.
   Body 14px (1..15).

   A bitmap, not geometry. The first cut was the m construction at stroke 2 /
   arch 1.5, and the round shoulders and round dot came out as half-strength
   pixels: a fuzzy top row and a dot at about 60% white, the one part that has
   to survive. So at this one size it is drawn the way a 16px icon is drawn by
   hand: legs 2px (3-4, 6-7, 9-10), counters 1px, shoulders two rows deep with
   the two outer corners at half strength for the curve, and the dot a solid
   2x2 on the baseline one clear pixel right of the last leg. 2px legs with
   1px counters is the nearest this grid gets to the master's 0.96
   stroke-to-arch without the m going light.
   Ink spans columns 3..13, so the right margin is a pixel narrower than the
   left: the dot is light and low, and centring the ink box exactly left the m
   looking pushed left. Rows 5..10, centred.
   '#' full, '+' half, '.' empty. */
export const ICON16 = {
  bodyInset: 1,
  rows: [
    "................",
    "................",
    "................",
    "................",
    "................",
    "...+######+.....",
    "...########.....",
    "...##.##.##.....",
    "...##.##.##.....",
    "...##.##.##.##..",
    "...##.##.##.##..",
    "................",
    "................",
    "................",
    "................",
    "................",
  ],
};

/* Bitmap rows -> horizontal runs { x, y, w, a } (a = coverage, 1 or 0.5). */
export function pixelRuns(rows) {
  const runs = [];
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; ) {
      const c = row[x];
      if (c === ".") {
        x++;
        continue;
      }
      let w = 1;
      while (row[x + w] === c) w++;
      runs.push({ x, y, w, a: c === "#" ? 1 : 0.5 });
      x += w;
    }
  });
  return runs;
}

/* Menu-bar template, 16x16, pure black with alpha; macOS tints it. Not scaled
   from the tile: drawn on the pixel grid so each leg is 2px on whole pixels.
   Counters are 2px here (there is no tile to fit inside), ink rows 4..11 and
   columns 1..14, centred both ways. The dot is 3px, a touch heavier than the
   legs as in the master, and its round edge leaves a clear pixel of white
   against the last leg at 1x. Every coordinate doubles onto whole pixels, so
   the 2x raster is the same drawing on the 32 grid, not an upscale. */
export const MENUBAR = {
  view: 16,
  stroke: 2,
  arch: 2,
  xtop: 5, // shoulders' ink top at 4
  baseline: 11, // leg ink ends at 12
  x0: 2,
  dot: { cx: 13.5, cy: 10.5, r: 1.5 },
};

/* Pixel-grid spec -> the same drawable parts as mdot(). */
export function mdotGrid(g) {
  const m = glyphs({ ...WORD, stroke: g.stroke, arch: g.arch, xtop: g.xtop, baseline: g.baseline }).m();
  return { stroke: g.stroke, paths: m.paths.map((d) => translate(d, g.x0)), dot: g.dot };
}

/* =============================================================================
   RETIRED — kept only so concepts.mjs and tune.mjs can still redraw the record
   of what was explored. Nothing that ships reads anything below this line.
   `[ · ]` was the adopted mark from 2026-09-27 (TUR-12) to 2026-09-30. The
   comments are left as they were when it was live.
   ============================================================================= */
/* -----------------------------------------------------------------------------
   DIRECTION B — "Brackets"  (adopted 2026-09-27, retired 2026-09-30)
   `[ · ]`. Two brackets hold the conversation; the space between them is
   deliberately empty, because nothing joins the call. The dot is the record
   light, and it is the only warm thing in the system.
-------------------------------------------------------------------------- */
/* Tuned against a 5-variant render at 200 / 32 / 18 / 16px, light and dark.
   corner 9 read as parentheses (soft, "listening"); corner 4 read hard against
   the Liquid Glass language. 5 is the point where it is unmistakably `[ ]` and
   still belongs beside a macOS control. Dot went 7.5 -> 10: at 7.5 it was the
   first thing to disappear at 32px. Stroke stayed at 11; 12.5 crowded the
   dot's diagonal clearance against the arm tips.

   Arms went 36/64 -> 31/69 after the viewfinder proximity probe
   (proximity.mjs and ../proofs/proof-proximity.png, both deleted with the
   bracket mark; recover them from the repository history). The gutter — the gap in the
   top and bottom edges — is the single knob that decides whether this reads as
   a bracket pair or as a camera focus target. Counter-intuitively, *widening*
   it moves away from the viewfinder: a viewfinder's signature is a closed
   square gestalt made of four disjoint corner marks, so the risk here came
   from the mark being too enclosed, not too open. Narrowing the gutter (tested
   at -10) closed it into a frame with a slot. Widening separates the two
   halves into unmistakable brackets. */
export const BRACKETS = {
  stroke: 11,
  stemL: 16,
  stemR: 84,
  top: 20,
  bottom: 80,
  corner: 5,
  armL: 31, // inner tip of the left bracket's arms
  armR: 69,
  dot: { cx: 50, cy: 50, r: 10 },
};

export function bracketPaths(g = BRACKETS) {
  const { stemL, stemR, top, bottom, corner: r, armL, armR } = g;
  const left =
    `M${armL},${top} H${f(stemL + r)} A${r},${r} 0 0 0 ${stemL},${f(top + r)} ` +
    `V${f(bottom - r)} A${r},${r} 0 0 0 ${f(stemL + r)},${bottom} H${armL}`;
  const right =
    `M${armR},${top} H${f(stemR - r)} A${r},${r} 0 0 1 ${stemR},${f(top + r)} ` +
    `V${f(bottom - r)} A${r},${r} 0 0 1 ${f(stemR - r)},${bottom} H${armR}`;
  return { left, right };
}

/* -----------------------------------------------------------------------------
   DIRECTION A — "Enclosure"
   A closed rounded square with a dot at its centre. The boundary is the idea:
   the recording never leaves the shape.
-------------------------------------------------------------------------- */
export const ENCLOSURE = { stroke: 11, half: 36, corner: 22, dot: 10 };

export function enclosurePath(g = ENCLOSURE) {
  const { half, corner: r } = g;
  const a = 50 - half;
  const b = 50 + half;
  return (
    `M${f(a + r)},${a} H${f(b - r)} A${r},${r} 0 0 1 ${b},${f(a + r)} ` +
    `V${f(b - r)} A${r},${r} 0 0 1 ${f(b - r)},${b} H${f(a + r)} ` +
    `A${r},${r} 0 0 1 ${a},${f(b - r)} V${f(a + r)} A${r},${r} 0 0 1 ${f(a + r)},${a} Z`
  );
}

/* -----------------------------------------------------------------------------
   DIRECTION C — "Turns"
   Two capsules, unequal and offset: the two captured tracks, and the
   turn-taking rhythm of a call. No dot — the shorter, warmer bar is "you".
-------------------------------------------------------------------------- */
export const TURNS = {
  stroke: 17,
  left: { x: 33, y1: 20, y2: 80 },
  right: { x: 67, y1: 40, y2: 66 },
};
