/* Renders the three exploratory directions as standalone SVGs plus one
   contact sheet. Kept in the repo as the record of what was explored. */
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  PALETTE as P,
  BRACKETS,
  bracketPaths,
  ENCLOSURE,
  enclosurePath,
  TURNS,
  squirclePath,
} from "./geometry.mjs";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "concepts");
mkdirSync(OUT, { recursive: true });

const wrap = (body, size = 100) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="${size}" height="${size}" fill="none">${body}</svg>`;

function markA(fg, accent) {
  const g = ENCLOSURE;
  return (
    `<path d="${enclosurePath(g)}" fill="none" stroke="${fg}" stroke-width="${g.stroke}" stroke-linejoin="round"/>` +
    `<circle cx="50" cy="50" r="${g.dot}" fill="${accent}"/>`
  );
}

function markB(fg, accent, g = BRACKETS) {
  const { left, right } = bracketPaths(g);
  const s = `fill="none" stroke="${fg}" stroke-width="${g.stroke}" stroke-linecap="round" stroke-linejoin="round"`;
  return (
    `<path d="${left}" ${s}/><path d="${right}" ${s}/>` +
    `<circle cx="${g.dot.cx}" cy="${g.dot.cy}" r="${g.dot.r}" fill="${accent}"/>`
  );
}

function markC(fg, accent) {
  const g = TURNS;
  const cap = `fill="none" stroke-width="${g.stroke}" stroke-linecap="round"`;
  return (
    `<path d="M${g.left.x},${g.left.y1} V${g.left.y2}" stroke="${fg}" ${cap}/>` +
    `<path d="M${g.right.x},${g.right.y1} V${g.right.y2}" stroke="${accent}" ${cap}/>`
  );
}

export const MARKS = { a: markA, b: markB, c: markC };

const DIRS = [
  ["a-enclosure", markA],
  ["b-brackets", markB],
  ["c-turns", markC],
];

for (const [name, fn] of DIRS) {
  writeFileSync(join(OUT, `meet-ai-concept-${name}-ember.svg`), wrap(fn(P.chalk, P.ember)));
  writeFileSync(join(OUT, `meet-ai-concept-${name}-mono.svg`), wrap(fn(P.ink, P.ink)));
}

/* ---- decision sheet ------------------------------------------------------
   Each direction in the app-icon squircle at 512 and at 16, plus the bare mark
   one-colour. This is the artefact the direction is chosen from. */
const sq = squirclePath(512, 512, 412);
const inIcon = (fn, px) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="${px}" height="${px}">
     <defs><linearGradient id="g${px}" x1="0" y1="0" x2="0" y2="1">
       <stop offset="0" stop-color="${P.inkTop}"/><stop offset="1" stop-color="${P.inkBottom}"/>
     </linearGradient></defs>
     <path d="${sq}" fill="url(#g${px})"/>
     <g transform="translate(512,512) scale(6.2) translate(-50,-50)">${fn(P.chalk, P.ember)}</g>
   </svg>`;

const NAMES = {
  "a-enclosure": ["A — Enclosure", "A closed boundary with the record light inside it: the recording never leaves the shape."],
  "b-brackets": ["B — Brackets", "<code>[ &middot; ]</code>. Two brackets hold the conversation and the space between them stays empty, because nothing joins the call."],
  "c-turns": ["C — Turns", "Two capsules, unequal and offset: the two captured tracks and the turn-taking rhythm of a call."],
};

const block = ([name, fn]) => `
  <section>
    <h2>${NAMES[name][0]}</h2><p class="note">${NAMES[name][1]}</p>
    <div class="strip">
      <div class="cell">${inIcon(fn, 256)}<span>app icon, 256</span></div>
      <div class="cell">${inIcon(fn, 32)}<span>32</span></div>
      <div class="cell">${inIcon(fn, 16)}<span>16</span></div>
      <div class="cell"><div class="box" style="background:${P.ink}">${wrap(fn(P.chalk, P.ember), 96)}</div><span>mark 96</span></div>
      <div class="cell"><div class="box" style="background:#fff">${wrap(fn("#000", "#000"), 96)}</div><span>one colour</span></div>
      <div class="cell"><div class="box" style="background:#fff">${wrap(fn("#000", "#000"), 16)}</div><span>one colour 16</span></div>
    </div>
  </section>`;

writeFileSync(
  join(OUT, "_directions.html"),
  `<!doctype html><meta charset="utf-8"><style>
    body{margin:0;padding:34px;background:#7e7f88;font:12px/1.5 -apple-system,system-ui;color:#fff}
    h2{font:600 14px -apple-system;margin:0 0 4px}
    .note{margin:0 0 14px;color:rgba(255,255,255,.75);font-size:11px;max-width:74ch}
    code{background:rgba(0,0,0,.28);padding:1px 5px;border-radius:4px}
    section{margin-bottom:30px}
    .strip{display:flex;align-items:flex-end;gap:22px}
    .cell{display:flex;flex-direction:column;align-items:center;gap:6px}
    .cell span{font-size:10px;color:rgba(255,255,255,.72)}
    .box{display:grid;place-items:center;padding:14px;border-radius:8px}
  </style>${DIRS.map(block).join("")}`,
);

console.log("concepts written to", OUT);
