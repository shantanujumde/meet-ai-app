/* =============================================================================
   Review proofs. Run after build.mjs + render.sh. Writes HTML into .render/
   (alongside the PNGs, so relative <img> works under file://) and render-proof.sh
   screenshots them.
   ============================================================================= */
import { writeFileSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { PALETTE as P, MENUBAR, bracketPaths } from "./geometry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const BRAND = join(HERE, "..");
const STAGE = join(HERE, ".render");
const svg = (rel) => readFileSync(join(BRAND, rel), "utf8").replace(/<\?xml.*?\?>/, "");
const sized = (rel, w, h) =>
  svg(rel).replace(/\swidth="[^"]*"/, ` width="${w}"`).replace(/\sheight="[^"]*"/, ` height="${h ?? ""}"`);

const CSS = `
 *{box-sizing:border-box}
 body{margin:0;padding:32px;background:#7e7f88;font:12px/1.5 -apple-system,system-ui;color:#fff}
 h2{font:600 14px -apple-system;margin:0 0 4px;letter-spacing:.01em}
 p.note{margin:0 0 14px;color:rgba(255,255,255,.72);font-size:11px;max-width:70ch}
 section{margin-bottom:30px}
 .row{display:flex;gap:18px;align-items:flex-end;flex-wrap:wrap}
 .c{display:flex;flex-direction:column;align-items:center;gap:5px}
 .c span{font-size:10px;color:rgba(255,255,255,.72)}
 .pad{padding:18px;border-radius:10px}
 .dark{background:${P.ink}} .dock{background:linear-gradient(#3d3e48,#23242b)}
 .light{background:#ECECF0} .paper{background:#fff}
 .light span,.paper span{color:rgba(0,0,0,.6)}
 img{image-rendering:pixelated;display:block}
`;

const page = (title, body) =>
  `<!doctype html><meta charset="utf-8"><title>${title}</title><style>${CSS}</style>${body}`;

/* --- 1. app icon at real sizes ------------------------------------------ */
{
  const sizes = [16, 24, 32, 48, 64, 128, 256, 512];
  const strip = (list) =>
    list.map((s) => `<div class="c"><img src="icon-${s}.png" width="${s}"><span>${s}</span></div>`).join("");
  writeFileSync(
    join(STAGE, "proof-icon.html"),
    page(
      "app icon",
      `<section><h2>App icon — Dock background</h2>
       <p class="note">Rendered PNGs at 1:1, no upscaling. 16-48px use the small-grid artwork; 64px and up use the primary.</p>
       <div class="row pad dock">${strip(sizes)}</div></section>
     <section><h2>App icon — light background (Finder, Launchpad)</h2>
       <div class="row pad light">${strip([16, 24, 32, 48, 64, 128, 256])}</div></section>
     <section><h2>Pixel inspection</h2>
       <p class="note">The same files magnified, to check that strokes land on whole pixels and the record dot stays a dot.</p>
       <div class="row pad dock">
         <div class="c"><img src="icon-16.png" width="160"><span>16px @10x</span></div>
         <div class="c"><img src="icon-32.png" width="160"><span>32px @5x</span></div>
         <div class="c"><img src="icon-48.png" width="160"><span>48px @3.3x</span></div>
         <div class="c"><img src="icon-64.png" width="160"><span>64px @2.5x</span></div>
       </div></section>`,
    ),
  );
}

/* --- 2. menu bar, the primary usage context ----------------------------- */
{
  const g = MENUBAR;
  const { left, right } = bracketPaths(g);
  const tmpl = (px, color) =>
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" width="${px}" height="${px}" fill="none">
      <path d="${left}" stroke="${color}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"/>
      <path d="${right}" stroke="${color}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"/>
      <circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${color}"/></svg>`;

  const bar = (bg, fg, label) => `
    <div class="c">
      <div style="width:520px;height:24px;background:${bg};display:flex;align-items:center;justify-content:flex-end;gap:14px;padding:0 12px;border-radius:0 0 6px 6px;font:11px -apple-system;color:${fg}">
        <span style="opacity:.8">100%</span>
        ${tmpl(16, fg)}
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" style="opacity:.75"><circle cx="7" cy="7" r="5" stroke="${fg}" stroke-width="1.6"/><path d="M11 11L14.5 14.5" stroke="${fg}" stroke-width="1.6" stroke-linecap="round"/></svg>
        <span style="opacity:.8">Sat 13:40</span>
      </div><span>${label}</span>
    </div>`;

  writeFileSync(
    join(STAGE, "proof-menubar.html"),
    page(
      "menu bar",
      `<section><h2>Menu bar — the primary usage context</h2>
        <p class="note">Template artwork at 16&times;16, drawn on the pixel grid. macOS tints a template image itself, so the mark carries no brand colour here; the record dot goes red from <code>--status-recording</code> only while recording.</p>
        <div class="row" style="flex-direction:column;align-items:flex-start;gap:14px">
          ${bar("#1c1c20", "rgba(255,255,255,.92)", "dark menu bar, idle")}
          ${bar("#f2f2f5", "rgba(0,0,0,.85)", "light menu bar, idle")}
        </div></section>
      <section><h2>Recording state</h2>
        <p class="note">Same geometry; only the dot changes. State is never carried by colour alone — the popover shows an elapsed timer beside it.</p>
        <div class="row" style="flex-direction:column;align-items:flex-start;gap:14px">
          <div class="c"><div style="width:520px;height:24px;background:#1c1c20;display:flex;align-items:center;justify-content:flex-end;gap:14px;padding:0 12px;font:11px -apple-system;color:rgba(255,255,255,.92)">
            <span style="opacity:.55;font-variant-numeric:tabular-nums">12:04</span>${tmpl(16, "#FF453A")}<span style="opacity:.8">Sat 13:40</span>
          </div><span>recording</span></div>
        </div></section>
      <section><h2>Template at 1x / 2x / inspection</h2>
        <div class="row pad dark">
          <div class="c">${tmpl(16, P.chalk)}<span>16 (1x)</span></div>
          <div class="c">${tmpl(32, P.chalk)}<span>32 (2x)</span></div>
          <div class="c">${tmpl(160, P.chalk)}<span>geometry</span></div>
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
      <div class="row pad dark">${sized("meet-ai-logo-horizontal-chalk.svg", 340)}</div>
      <div class="row pad paper" style="margin-top:12px">${sized("meet-ai-logo-horizontal-ink.svg", 340)}</div>
     </section>
   <section><h2>Lockup — one colour</h2>
      <p class="note">Flattened to pure black and pure white before any colour version was approved.</p>
      <div class="row pad paper">${sized("meet-ai-logo-horizontal-mono-black.svg", 300)}</div>
      <div class="row pad" style="background:#000;margin-top:12px">${sized("meet-ai-logo-horizontal-mono-white.svg", 300)}</div>
     </section>
   <section><h2>Lockup — minimum size</h2>
      <div class="row pad dark">
        <div class="c">${sized("meet-ai-logo-horizontal-chalk.svg", 120)}<span>120px — minimum</span></div>
        <div class="c">${sized("meet-ai-logo-horizontal-chalk.svg", 80)}<span>80px — below minimum, the e counters fill in</span></div>
      </div></section>
   <section><h2>Symbol</h2>
      <div class="row pad dark">
        ${[128, 64, 40].map((s) => `<div class="c">${sized("meet-ai-logomark-primary-chalk.svg", s, s)}<span>${s}</span></div>`).join("")}
        ${[32, 16].map((s) => `<div class="c">${sized("meet-ai-logomark-small-chalk.svg", s, s)}<span>${s} (small grid)</span></div>`).join("")}
      </div>
      <div class="row pad paper" style="margin-top:12px">
        ${[128, 64, 40].map((s) => `<div class="c">${sized("meet-ai-logomark-mono-black.svg", s, s)}<span>${s}</span></div>`).join("")}
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
        <div style="background:linear-gradient(160deg,#20222A,#121317);padding:44px 48px">
          ${sized("meet-ai-logo-horizontal-chalk.svg", 258)}
          <p style="margin:18px 0 0;font:400 15px/1.55 -apple-system;color:rgba(255,255,255,.66);max-width:56ch">
            A botless meeting recorder for macOS. Nothing joins your call. The audio never leaves your machine.
          </p>
        </div>
        <div style="background:#fff;color:#1c1c20;padding:26px 48px;font:13px/1.6 -apple-system">
          <code style="background:#f2f2f5;padding:3px 7px;border-radius:5px">just dev</code>
        </div>
      </div></section>
    <section><h2>Window title bar</h2>
      <div style="width:560px;border-radius:12px;overflow:hidden;box-shadow:0 16px 48px rgba(0,0,0,.4)">
        <div style="background:rgba(40,41,48,.96);height:38px;display:flex;align-items:center;padding:0 14px;gap:8px">
          <span style="width:12px;height:12px;border-radius:99px;background:#FF5F57"></span>
          <span style="width:12px;height:12px;border-radius:99px;background:#FEBC2E"></span>
          <span style="width:12px;height:12px;border-radius:99px;background:#28C840"></span>
          <div style="flex:1;display:flex;align-items:center;justify-content:center;gap:7px">
            ${sized("meet-ai-logomark-small-chalk.svg", 14, 14)}
            <span style="font:600 13px -apple-system;color:rgba(255,255,255,.88)">meet-ai</span>
          </div>
          <span style="width:56px"></span>
        </div>
        <div style="background:rgba(28,28,32,.94);height:120px"></div>
      </div></section>`,
  ),
);

/* --- 5. misuse ----------------------------------------------------------- */
{
  const m = (s, label, note) =>
    `<div class="c" style="align-items:flex-start">
       <div class="pad" style="background:${P.ink};width:200px;height:150px;display:grid;place-items:center;position:relative">
         <div style="${s}">${sized("meet-ai-logomark-primary-chalk.svg", 76, 76)}</div>
         <span style="position:absolute;top:8px;right:10px;font-size:15px;color:#FF453A">&#10006;</span>
       </div>
       <b style="font-size:11px">${label}</b><span style="max-width:200px;text-align:left">${note}</span>
     </div>`;
  writeFileSync(
    join(STAGE, "proof-misuse.html"),
    page(
      "misuse",
      `<section><h2>Correct</h2>
        <div class="row">
          <div class="c" style="align-items:flex-start"><div class="pad" style="background:${P.ink};width:200px;height:150px;display:grid;place-items:center">${sized("meet-ai-logomark-primary-chalk.svg", 76, 76)}</div><b style="font-size:11px">On ink</b></div>
          <div class="c" style="align-items:flex-start"><div class="pad" style="background:#fff;width:200px;height:150px;display:grid;place-items:center">${sized("meet-ai-logomark-primary-ink.svg", 76, 76)}</div><b style="font-size:11px;color:#fff">On paper</b></div>
          <div class="c" style="align-items:flex-start"><div class="pad" style="background:#000;width:200px;height:150px;display:grid;place-items:center">${sized("meet-ai-logomark-mono-white.svg", 76, 76)}</div><b style="font-size:11px">One colour</b></div>
        </div></section>
      <section><h2>Do not</h2>
        <div class="row">
          ${m("transform:scaleX(1.45)", "Stretch it", "The stroke weight stops being uniform and the dot becomes an ellipse.")}
          ${m("filter:hue-rotate(190deg) saturate(1.6)", "Recolour the dot", "The dot is the record light. Ember or the recording red, nothing else.")}
          ${m("filter:drop-shadow(0 10px 0 rgba(255,138,60,.9))", "Add effects", "No long shadows, no bevels, no outer glow.")}
          ${m("transform:rotate(-16deg)", "Rotate it", "The brackets read as brackets only when they are upright.")}
        </div>
        <div class="row" style="margin-top:16px">
          <div class="c" style="align-items:flex-start">
            <div class="pad" style="width:200px;height:150px;display:grid;place-items:center;position:relative;background:linear-gradient(45deg,#FF8A3C,#8A5CF6,#25C2A0);">
              ${sized("meet-ai-logomark-primary-chalk.svg", 76, 76)}
              <span style="position:absolute;top:8px;right:10px;font-size:15px;color:#fff">&#10006;</span>
            </div><b style="font-size:11px">Busy background</b><span style="max-width:200px">Put the mark on ink, paper, or a flat neutral. Never on a gradient or a photograph.</span>
          </div>
          <div class="c" style="align-items:flex-start">
            <div class="pad" style="background:${P.ink};width:200px;height:150px;display:grid;place-items:center;position:relative">
              <div style="display:flex;align-items:center;gap:2px">${sized("meet-ai-logomark-primary-chalk.svg", 50, 50)}<span style="font:600 20px -apple-system">meet-ai</span></div>
              <span style="position:absolute;top:8px;right:10px;font-size:15px;color:#FF453A">&#10006;</span>
            </div><b style="font-size:11px">Rebuild the lockup</b><span style="max-width:200px">Use the supplied lockup file. Do not set the name in a system font next to the mark.</span>
          </div>
          <div class="c" style="align-items:flex-start">
            <div class="pad" style="background:${P.ink};width:200px;height:150px;display:grid;place-items:center;position:relative">
              <div style="display:flex;gap:0">${sized("meet-ai-logomark-primary-chalk.svg", 76, 76)}<div style="width:76px;height:76px;background:#3A3C46;border-radius:8px"></div></div>
              <span style="position:absolute;top:8px;right:10px;font-size:15px;color:#FF453A">&#10006;</span>
            </div><b style="font-size:11px">Crowd it</b><span style="max-width:200px">Clear space on every side is the height of one bracket stem.</span>
          </div>
        </div></section>`,
    ),
  );
}

console.log("proofs written to", STAGE);
