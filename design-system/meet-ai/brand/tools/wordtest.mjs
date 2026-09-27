import { writeFileSync } from "node:fs";
import { PALETTE as P, layoutWordmark } from "./geometry.mjs";

const w = layoutWordmark();
const svg = (fg, accent, h) => {
  const pad = w.stroke / 2 + 1;
  const vb = `${-pad} ${8} ${w.width + pad * 2} ${72 - 8 + pad}`;
  const scale = h / (72 - 8 + pad);
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vb}" width="${Math.round(
    (w.width + pad * 2) * scale,
  )}" height="${h}" fill="none">
    ${w.paths
      .map(
        (d) =>
          `<path d="${d}" stroke="${fg}" stroke-width="${w.stroke}" stroke-linecap="round" stroke-linejoin="round"/>`,
      )
      .join("")}
    ${w.dots
      .map((d) => `<circle cx="${d.cx}" cy="${d.cy}" r="${d.r}" fill="${d.accent ? accent : fg}"/>`)
      .join("")}
  </svg>`;
};

writeFileSync(
  new URL("../concepts/_word.html", import.meta.url),
  `<!doctype html><meta charset="utf-8"><style>
   body{margin:0;padding:28px;background:#909098;font:11px -apple-system;color:#fff}
   .b{background:${P.ink};padding:22px;border-radius:8px;margin-bottom:12px}
   .l{background:#fff;padding:22px;border-radius:8px;margin-bottom:12px}
   .row{display:flex;gap:20px;align-items:baseline;flex-wrap:wrap}
  </style>
  <div class="b"><div class="row">${[120, 64, 34, 20, 14]
    .map((h) => svg(P.chalk, P.ember, h))
    .join("")}</div></div>
  <div class="l"><div class="row">${[120, 64, 34, 20, 14]
    .map((h) => svg("#111", P.ember, h))
    .join("")}</div></div>
  <div class="l"><div class="row">${[120, 34].map((h) => svg("#000", "#000", h)).join("")}</div></div>`,
);
console.log("width", w.width);
