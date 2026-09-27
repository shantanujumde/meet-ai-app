/* =============================================================================
   meet-ai brand — geometry source of truth
   Every SVG in ../ is generated from this file. Edit here, run `build.mjs`,
   never hand-edit an output SVG.

   Coordinate system: a 100 x 100 artboard. The mark is optically centred on
   (50, 50). All stroke geometry is expressed on the *centreline*, so the
   visual bounding box is the centreline box grown by half a stroke.
   ============================================================================= */

export const PALETTE = {
  inkTop: "#24262E",
  inkBottom: "#121317",
  ink: "#16181D", // flat one-colour ink
  chalk: "#F4F5F7", // the mark on dark
  emberCore: "#FFC152",
  ember: "#FF8A3C", // primary brand colour
  emberRim: "#E85F21",
  emberInk: "#A8410D", // accessible-on-white text variant
};

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
   DIRECTION B — "Brackets"  (the recommended mark)
   `[ · ]`. Two brackets hold the conversation; the space between them is
   deliberately empty, because nothing joins the call. The dot is the record
   light, and it is the only warm thing in the system.
-------------------------------------------------------------------------- */
/* Tuned against a 5-variant render at 200 / 32 / 18 / 16px, light and dark.
   corner 9 read as parentheses (soft, "listening"); corner 4 read hard against
   the Liquid Glass language. 5 is the point where it is unmistakably `[ ]` and
   still belongs beside a macOS control. Dot went 7.5 -> 10: at 7.5 it was the
   first thing to disappear at 32px. Stroke stayed at 11; 12.5 crowded the
   dot's diagonal clearance against the arm tips. */
export const BRACKETS = {
  stroke: 11,
  stemL: 16,
  stemR: 84,
  top: 20,
  bottom: 80,
  corner: 5,
  armL: 36, // inner tip of the left bracket's arms
  armR: 64,
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

/* Small-size variant, for raster exports at or below 32px. Scaling the primary
   mark down puts its stroke on a fractional pixel and lets the dot fill in.
   This one is redrawn heavier and more open so that after the squircle's
   inset there is still a whole pixel of gutter around the dot. */
export const BRACKETS_SMALL = {
  stroke: 15,
  stemL: 18,
  stemR: 82,
  top: 22,
  bottom: 78,
  corner: 4,
  armL: 35,
  armR: 65,
  dot: { cx: 50, cy: 50, r: 12 },
};

/* Menu-bar template. Not derived from the 100-unit grid at all: drawn directly
   on the 16px pixel grid so every stroke lands on whole pixels and nothing is
   antialiased into grey. This is the app's primary usage context. */
export const MENUBAR = {
  view: 16,
  stroke: 2,
  stemL: 2,
  stemR: 14,
  top: 3,
  bottom: 13,
  corner: 1,
  armL: 5,
  armR: 11,
  dot: { cx: 8, cy: 8, r: 2 },
};

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

/* -----------------------------------------------------------------------------
   WORDMARK — "meet-ai"
   Drawn, not set. No licensed typeface is involved, and the letterforms reuse
   the mark's own monoline construction: one stroke weight, round terminals,
   circular bowls. The `i` tittle is the record dot, which is what ties the
   wordmark to the symbol.

   Baseline 67.5 (centreline), x-height top 32.5 (centreline), ascender 18.5.
   Letter stroke is 9, one unit lighter than the mark's 11 — a monoline text
   stroke reads heavier than the same weight in a symbol.
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
function translate(d, dx) {
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

/* App icon drawn directly on the 16px pixel grid. Scaling the 1024 master down
   to 16 leaves every stroke straddling a pixel boundary and the record dot
   dissolves into grey. Here the body is 14px (1..15), strokes are 2px on whole
   pixels, and the dot is a clean 2px. */
export const ICON16 = {
  bodyInset: 1,
  stroke: 2,
  stemL: 4,
  stemR: 12,
  top: 5,
  bottom: 11,
  corner: 1,
  armL: 6,
  armR: 10,
  dot: { cx: 8, cy: 8, r: 1 },
};
