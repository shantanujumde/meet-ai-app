/* =============================================================================
   Review proofs. Run after build.mjs + render.sh. Writes HTML into .render/
   (alongside the PNGs, so relative <img> works under file://) and render-proof.sh
   screenshots them.

   Every icon and menu-bar image on these pages is a file that ships, or an
   ictool render of the .icon that ships — not a fresh drawing of the geometry.
   A proof that redraws the artwork can only show that the geometry is right;
   it cannot show that the pipeline delivered it.
   ============================================================================= */
import { writeFileSync, readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { PALETTE as P, MDOT, mdot } from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BRAND = join(HERE, "..");
const STAGE = join(HERE, ".render");
const svg = (rel) => readFileSync(join(BRAND, rel), "utf8").replace(/<\?xml.*?\?>/, "");
const sized = (rel, w, h) =>
  svg(rel).replace(/\swidth="[^"]*"/, ` width="${w}"`).replace(/\sheight="[^"]*"/, ` height="${h ?? ""}"`);

const CSS = `
 *{box-sizing:border-box}
 body{margin:0;padding:32px;background:#7e7f88;font:12px/1.5 -apple-system,system-ui;color:#fff}
 h2{font:600 14px -apple-system,system-ui,sans-serif;margin:0 0 4px;letter-spacing:.01em}
 p.note{margin:0 0 14px;color:rgba(255,255,255,.72);font-size:11px;max-width:78ch}
 section{margin-bottom:30px}
 .row{display:flex;gap:18px;align-items:flex-end;flex-wrap:wrap}
 .c{display:flex;flex-direction:column;align-items:center;gap:5px}
 .c span{font-size:10px;color:rgba(255,255,255,.72)}
 .pad{padding:18px;border-radius:10px}
 .dark{background:${P.ink}} .dock{background:linear-gradient(#3d3e48,#23242b)}
 .light{background:#ECECF0} .paper{background:#fff}
 .light span,.paper span{color:rgba(0,0,0,.6)}
 img{image-rendering:pixelated;display:block}
 img.smooth{image-rendering:auto}
 code{background:rgba(0,0,0,.25);padding:1px 5px;border-radius:4px}
`;

const page = (title, body) =>
  `<!doctype html><meta charset="utf-8"><title>${title}</title><style>${CSS}</style>${body}`;

/* render-proof.sh screenshots at device scale 2, so an <img> laid out at N CSS
   px is drawn with 2N device pixels. `at1x` lays a raster out at half its
   pixel width, which puts it on screen 1:1 — exactly as a Retina display shows
   that file. */
const at1x = (file, px, label, cls = "") =>
  `<div class="c"><img class="${cls}" src="${file}" width="${px / 2}"><span>${label}</span></div>`;

/* --- 1. app icon at real sizes ------------------------------------------ */
{
  const legacy = [16, 24, 32, 48, 64, 128, 256, 512];
  const strip = (list) =>
    list.map((s) => at1x(`icon-${s}.png`, s, `${s}px`)).join("");

  // ictool renders of the shipped meet-ai.icon (render-proof.sh makes them).
  const APPEAR = ["Default", "Dark", "TintedLight", "TintedDark", "ClearLight", "ClearDark"];
  const have = existsSync(join(STAGE, "app-Default-128@2x.png"));
  const composer = have
    ? `<section><h2>meet-ai.icon — what macOS 26 draws (Dock, Finder, Launchpad)</h2>
       <p class="note">Rendered by Icon Composer's own <code>ictool</code> from the shipped document, the source of <code>Assets.car</code>. On macOS 26 this, not the <code>.icns</code>, is the icon. The system draws the squircle, the edge light and the shadow; the tile is Dusk and the glyph is flat white, glass off. 1:1 on a Retina display.</p>
       <div class="row pad dock">${[16, 32, 64, 128, 256].map((s) => at1x(`app-Default-${s}@2x.png`, s * 2, `${s}pt`, "smooth")).join("")}</div>
       <div class="row pad light" style="margin-top:12px">${[16, 32, 64, 128].map((s) => at1x(`app-Default-${s}@2x.png`, s * 2, `${s}pt`, "smooth")).join("")}</div></section>
     <section><h2>meet-ai.icon — every appearance</h2>
       <p class="note">Dark: the tile turns to the brand's ink and the colour moves into the glyph; the dot is coral, the record light. Tinted and clear: the system's one-colour renderings, the glyph in plain white.</p>
       <div class="row pad dock">${APPEAR.map((r) => at1x(`app-${r}-128@2x.png`, 256, r, "smooth")).join("")}</div>
       <div class="row pad dock" style="margin-top:12px">${APPEAR.map((r) => at1x(`app-${r}-32@2x.png`, 64, `${r} 32pt`, "smooth")).join("")}</div></section>
     <section><h2>meet-ai.icon — the small end, magnified</h2>
       <p class="note">The format has one drawing for every size, so the 16pt and 32pt icons are the system's own reduction of the 1024 master. The check: three legs, two open counters, and the dot a separate full stop.</p>
       <div class="row pad dock">
         ${["Default", "Dark", "TintedLight"].map((r) => `<div class="c"><img src="app-${r}-16@1x.png" width="128"><span>${r} 16px @8x</span></div>`).join("")}
         ${["Default", "Dark"].map((r) => `<div class="c"><img src="app-${r}-32@1x.png" width="128"><span>${r} 32px @4x</span></div>`).join("")}
       </div></section>`
    : `<section><h2>meet-ai.icon</h2><p class="note">Not shown: ictool (Xcode) was not found when render-proof.sh ran.</p></section>`;

  writeFileSync(
    join(STAGE, "proof-icon.html"),
    page(
      "app icon",
      `${composer}
     <section><h2>Legacy rasters — icon.icns, 32x32.png, favicons, .ico</h2>
       <p class="note">The files render.sh ships, 1:1. 16px is drawn pixel by pixel; 24–64px use the small artwork files, which now carry the master's geometry; 128px and up use the master. On macOS 26 the <code>.icns</code> is only the fallback for <code>Assets.car</code>.</p>
       <div class="row pad dock">${strip(legacy)}</div>
       <div class="row pad light" style="margin-top:12px">${strip([16, 24, 32, 48, 64, 128, 256])}</div></section>
     <section><h2>Legacy rasters — pixel inspection</h2>
       <p class="note">The same files magnified, to check the legs stay legs and the dot stays a dot.</p>
       <div class="row pad dock">
         <div class="c"><img src="icon-16.png" width="160"><span>16px @10x</span></div>
         <div class="c"><img src="icon-24.png" width="160"><span>24px @6.7x</span></div>
         <div class="c"><img src="icon-32.png" width="160"><span>32px @5x</span></div>
         <div class="c"><img src="icon-64.png" width="160"><span>64px @2.5x</span></div>
       </div></section>`,
    ),
  );
}

/* --- 2. menu bar, the primary usage context ----------------------------- */
/* The images here are the two rasters render.sh copies into src-tauri/icons/.
   The @2x one is what tray.rs embeds, so on a Retina menu bar it is the only
   one that is ever drawn. A template is black with alpha and AppKit tints it:
   dark on a light bar, light on a dark one. invert() is that tint, near
   enough — AppKit also lowers the glyph's opacity a little on a dark bar. */
{
  const bar = (bg, fg, dark, label) => `
    <div class="c" style="align-items:flex-start">
      <div style="width:560px;height:24px;background:${bg};display:flex;align-items:center;justify-content:flex-end;gap:14px;padding:0 12px;border-radius:0 0 6px 6px;font:13px -apple-system,system-ui,sans-serif;color:${fg}">
        <b style="font-weight:400;opacity:.85">100%</b>
        <img src="tray-32.png" width="18" style="image-rendering:auto;${dark ? "filter:invert(1);opacity:.92" : "opacity:.85"}">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" style="opacity:.8"><circle cx="7" cy="7" r="5" stroke="${fg}" stroke-width="1.6"/><path d="M11 11L14.5 14.5" stroke="${fg}" stroke-width="1.6" stroke-linecap="round"/></svg>
        <b style="font-weight:400;opacity:.85">Wed 13:40</b>
      </div><span>${label}</span>
    </div>`;

  writeFileSync(
    join(STAGE, "proof-menubar.html"),
    page(
      "menu bar",
      `<section><h2>Menu bar — the primary usage context</h2>
        <p class="note"><code>meet-aiTemplate@2x.png</code>, the file <code>tray.rs</code> embeds, at the 18pt height the tray-icon crate draws it on a Retina bar. Black with alpha: macOS tints it, so it carries no brand colour, and it has no recording state — the popover and the menu say what is happening.</p>
        <div class="row" style="flex-direction:column;align-items:flex-start;gap:14px">
          ${bar("#1c1c20", "rgba(255,255,255,.92)", true, "dark menu bar")}
          ${bar("#f2f2f5", "rgba(0,0,0,.85)", false, "light menu bar")}
        </div></section>
      <section><h2>Template at 1x / 2x</h2>
        <p class="note">Both drawn from one 16px-grid drawing: legs 2px on whole pixels, counters 2px, a 3px dot one clear pixel from the last leg. The 2x is the same geometry at twice the grid, not an upscale.</p>
        <div class="row pad light">
          <div class="c"><img src="tray-16.png" width="16"><span>1x, 16px (as a non-Retina bar draws it)</span></div>
          <div class="c"><img src="tray-32.png" width="16" style="image-rendering:auto"><span>2x, 32px (Retina)</span></div>
          <div class="c"><img src="tray-16.png" width="160"><span>1x @10x</span></div>
          <div class="c"><img src="tray-32.png" width="160"><span>2x @5x</span></div>
        </div>
        <div class="row pad dark" style="margin-top:12px">
          <div class="c"><img src="tray-16.png" width="16" style="filter:invert(1)"><span>1x, tinted light</span></div>
          <div class="c"><img src="tray-32.png" width="16" style="filter:invert(1);image-rendering:auto"><span>2x, tinted light</span></div>
          <div class="c"><img src="tray-16.png" width="160" style="filter:invert(1)"><span>1x @10x</span></div>
          <div class="c"><img src="tray-32.png" width="160" style="filter:invert(1)"><span>2x @5x</span></div>
        </div></section>`,
    ),
  );
}

/* --- 3. logo system ------------------------------------------------------ */
writeFileSync(
  join(STAGE, "proof-logo.html"),
  page(
    "logo system",
    `<section><h2>Horizontal lockup</h2>
      <p class="note">"m." set to the word's full height, baselines level: the dot is a full stop, so it sits on the line the word sits on.</p>
      <div class="row pad dark">${sized("meet-ai-logo-horizontal-chalk.svg", 360)}</div>
      <div class="row pad paper" style="margin-top:12px">${sized("meet-ai-logo-horizontal-ink.svg", 360)}</div>
     </section>
   <section><h2>Lockup — one colour</h2>
      <div class="row pad paper">${sized("meet-ai-logo-horizontal-mono-black.svg", 300)}</div>
      <div class="row pad" style="background:#000;margin-top:12px">${sized("meet-ai-logo-horizontal-mono-white.svg", 300)}</div>
     </section>
   <section><h2>Lockup — minimum size</h2>
      <div class="row pad dark">
        <div class="c">${sized("meet-ai-logo-horizontal-chalk.svg", 120)}<span>120px — minimum</span></div>
        <div class="c">${sized("meet-ai-logo-horizontal-chalk.svg", 80)}<span>80px — below minimum, the e counters fill in</span></div>
      </div></section>
   <section><h2>Symbol</h2>
      <p class="note">Off the tile: the m in chalk or ink, the dot in ember — the same accent as the wordmark's i. On the tile it is all white (see the app icon proof).</p>
      <div class="row pad dark">
        ${[128, 64, 40].map((s) => `<div class="c">${sized("meet-ai-logomark-primary-chalk.svg", s, s)}<span>${s}</span></div>`).join("")}
        ${[32, 20].map((s) => `<div class="c">${sized("meet-ai-logomark-small-chalk.svg", s, s)}<span>${s} (small)</span></div>`).join("")}
      </div>
      <div class="row pad paper" style="margin-top:12px">
        ${[128, 64, 40].map((s) => `<div class="c">${sized("meet-ai-logomark-primary-ink.svg", s, s)}<span>${s}</span></div>`).join("")}
        ${[128, 64, 40].map((s) => `<div class="c">${sized("meet-ai-logomark-mono-black.svg", s, s)}<span>${s} one colour</span></div>`).join("")}
      </div></section>
   <section><h2>Wordmark</h2>
      <div class="row pad dark">${sized("meet-ai-wordmark-primary-chalk.svg", 260)}</div>
      <div class="row pad paper" style="margin-top:12px">${sized("meet-ai-wordmark-primary-ink.svg", 260)}</div>
     </section>`,
  ),
);

/* --- 4. README header, an in-context render ------------------------------ */
writeFileSync(
  join(STAGE, "proof-readme.html"),
  page(
    "readme header",
    `<section><h2>README header</h2>
      <div style="width:900px;border-radius:12px;overflow:hidden;border:1px solid rgba(255,255,255,.14)">
        <div style="background:linear-gradient(160deg,#20222A,#121317);padding:44px 48px;display:flex;align-items:center;gap:28px">
          <img class="smooth" src="icon-256.png" width="96">
          <div>
            ${sized("meet-ai-logo-horizontal-chalk.svg", 258)}
            <p style="margin:16px 0 0;font:400 15px/1.55 -apple-system,system-ui,sans-serif;color:rgba(255,255,255,.66);max-width:56ch">
              A botless meeting recorder for macOS. Nothing joins your call. The audio never leaves your machine.
            </p>
          </div>
        </div>
        <div style="background:#fff;color:#1c1c20;padding:26px 48px;font:13px/1.6 -apple-system,system-ui,sans-serif">
          <code style="background:#f2f2f5;padding:3px 7px;border-radius:5px;color:#1c1c20">just dev</code>
        </div>
      </div></section>
    <section><h2>Window title bar</h2>
      <div style="width:560px;border-radius:12px;overflow:hidden;box-shadow:0 16px 48px rgba(0,0,0,.4)">
        <div style="background:rgba(40,41,48,.96);height:38px;display:flex;align-items:center;padding:0 14px;gap:8px">
          <span style="width:12px;height:12px;border-radius:99px;background:#FF5F57"></span>
          <span style="width:12px;height:12px;border-radius:99px;background:#FEBC2E"></span>
          <span style="width:12px;height:12px;border-radius:99px;background:#28C840"></span>
          <div style="flex:1;display:flex;align-items:center;justify-content:center;gap:7px">
            <img class="smooth" src="icon-32.png" width="16">
            <span style="font:600 13px -apple-system,system-ui,sans-serif;color:rgba(255,255,255,.88)">meet-ai</span>
          </div>
          <span style="width:56px"></span>
        </div>
        <div style="background:rgba(28,28,32,.94);height:120px"></div>
      </div></section>`,
  ),
);

/* --- 5. misuse ----------------------------------------------------------- */
{
  const tile = (inner, style = "") =>
    `<div style="width:120px;height:120px;border-radius:27px;background:linear-gradient(${P.tileTop},${P.tileBottom});display:grid;place-items:center;overflow:hidden;${style}">${inner}</div>`;
  const white = (w) => sized("meet-ai-logomark-mono-white.svg", w, w);
  // "m" with its dot lifted to where an i's tittle would be: Xiaomi's "mi".
  // Drawn from the real construction, so it tracks MDOT: the dot's column
  // becomes an i stem from just under the x-height to the baseline, and the
  // dot sits above the x-height.
  const g = mdot(MDOT);
  const iStem = `M${g.dot.cx},${MDOT.xtop + 124}V${MDOT.baseline}`;
  const tittle = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="108 108 808 808" width="92" height="92"><path d="${g.paths.join(" ")} ${iStem}" fill="none" stroke="#fff" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"/><circle cx="${g.dot.cx}" cy="${MDOT.xtop - 26}" r="${g.dot.r}" fill="#fff"/></svg>`;
  const cell = (body, label, note, bad = true) =>
    `<div class="c" style="align-items:flex-start;width:200px">
       <div class="pad" style="background:#5d5e66;width:200px;height:160px;display:grid;place-items:center;position:relative">
         ${body}
         ${bad ? `<span style="position:absolute;top:8px;right:10px;font-size:15px;color:#FF453A">&#10006;</span>` : ""}
       </div>
       <b style="font-size:11px">${label}</b><span style="max-width:200px;text-align:left">${note}</span>
     </div>`;
  writeFileSync(
    join(STAGE, "proof-misuse.html"),
    page(
      "misuse",
      `<section><h2>Correct</h2>
        <div class="row">
          ${cell(`<img class="smooth" src="icon-256.png" width="128">`, "App icon", "Dusk tile, white m., nothing else on it.", false)}
          ${cell(`<div class="pad dark" style="padding:22px">${sized("meet-ai-logomark-primary-chalk.svg", 84, 84)}</div>`, "On dark", "Chalk m, ember dot.", false)}
          ${cell(`<div class="pad paper" style="padding:22px">${sized("meet-ai-logomark-primary-ink.svg", 84, 84)}</div>`, "On light", "Ink m, ember dot.", false)}
        </div></section>
      <section><h2>Do not</h2>
        <div class="row" style="align-items:flex-start">
          ${cell(tile(white(92), "transform:scaleX(1.4)"), "Stretch it", "The stroke stops being uniform and the dot becomes an ellipse.")}
          ${cell(`<div style="width:120px;height:120px;border-radius:27px;background:linear-gradient(#3AA0F4,#3C4BF2);display:grid;place-items:center">${white(92)}</div>`, "Recolour the tile", "Dusk is the identity. Blue and purple are the meeting-notes field's colours — the reason it was not chosen.")}
          ${cell(tile(`<div style="filter:drop-shadow(0 8px 6px rgba(0,0,0,.45))">${white(92)}</div>`), "Add effects", "No drop shadow, bevel, glow or glass on the glyph.")}
          ${cell(tile(tittle), "Move the dot", "The dot is a full stop on the baseline. Lifted to where an i's tittle sits, it spells a white &ldquo;mi&rdquo; — Xiaomi's mark.")}
        </div>
        <div class="row" style="margin-top:16px;align-items:flex-start">
          ${cell(tile(white(92), "transform:rotate(-14deg)"), "Rotate it", "It is a letter; it reads upright.")}
          ${cell(`<div style="display:flex;align-items:center;gap:6px">${tile(white(56), "width:72px;height:72px;border-radius:16px")}<span style="font:600 20px -apple-system,system-ui,sans-serif">meet-ai</span></div>`, "Rebuild the lockup", "Use the supplied file. Do not set the name in a system font beside the mark.")}
          ${cell(`<div style="display:flex">${tile(white(92))}<div style="width:60px;height:120px;background:#3A3C46;border-radius:8px"></div></div>`, "Crowd it", "Clear space on every side is the diameter of the dot.")}
          ${cell(`<div class="pad" style="background:linear-gradient(45deg,#FF8A3C,#25C2A0,#8A5CF6);padding:22px">${sized("meet-ai-logomark-primary-chalk.svg", 84, 84)}</div>`, "Busy background", "Off the tile, the symbol goes on ink, paper or a flat neutral — not a gradient or a photograph.")}
        </div></section>`,
    ),
  );
}

console.log("proofs written to", STAGE);
