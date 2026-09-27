/* Scratch harness: bracket geometry variants side by side at judging sizes. */
import { writeFileSync } from "node:fs";
import { PALETTE as P, bracketPaths } from "./geometry.mjs";

const variants = [
  { name: "v0", stroke: 11, stemL: 15.5, stemR: 84.5, top: 21.5, bottom: 78.5, corner: 9, armL: 31, armR: 69, dot: { cx: 50, cy: 50, r: 7.5 } },
  { name: "v1", stroke: 11, stemL: 15.5, stemR: 84.5, top: 20, bottom: 80, corner: 6, armL: 35, armR: 65, dot: { cx: 50, cy: 50, r: 9.5 } },
  { name: "v2", stroke: 11, stemL: 16, stemR: 84, top: 20, bottom: 80, corner: 4, armL: 36, armR: 64, dot: { cx: 50, cy: 50, r: 10 } },
  { name: "v3", stroke: 12.5, stemL: 17, stemR: 83, top: 21, bottom: 79, corner: 5, armL: 36, armR: 64, dot: { cx: 50, cy: 50, r: 10.5 } },
  { name: "v4", stroke: 13.5, stemL: 18, stemR: 82, top: 23, bottom: 77, corner: 4, armL: 35, armR: 65, dot: { cx: 50, cy: 50, r: 11 } },
];

const svg = (g, fg, accent, size) => {
  const { left, right } = bracketPaths(g);
  const s = `stroke="${fg}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="${size}" height="${size}" fill="none"><path d="${left}" ${s}/><path d="${right}" ${s}/><circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${accent}"/></svg>`;
};

const cell = (g, size, fg, accent, bg) =>
  `<div class="c" style="width:${size + 14}px;height:${size + 14}px;background:${bg}">${svg(g, fg, accent, size)}</div>`;

const col = (g) =>
  `<div class="col"><b>${g.name}</b>
     ${cell(g, 200, P.chalk, P.ember, P.ink)}
     <div class="r">${cell(g, 32, P.chalk, P.ember, P.ink)}${cell(g, 18, P.chalk, P.ember, P.ink)}${cell(g, 16, P.chalk, P.ember, P.ink)}</div>
     <div class="r">${cell(g, 32, "#111", "#111", "#fff")}${cell(g, 18, "#111", "#111", "#fff")}${cell(g, 16, "#111", "#111", "#fff")}</div>
   </div>`;

writeFileSync(
  new URL("../concepts/_tune.html", import.meta.url),
  `<!doctype html><meta charset="utf-8"><style>
    body{margin:0;padding:20px;background:#909098;font:11px -apple-system;color:#fff;display:flex;gap:16px}
    .col{display:flex;flex-direction:column;gap:8px;align-items:center}
    .r{display:flex;gap:8px;align-items:center}
    .c{display:grid;place-items:center;border-radius:5px}
  </style>${variants.map(col).join("")}`,
);
console.log("ok");
