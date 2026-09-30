/**
 * Join class names, with later Tailwind utilities winning over earlier ones.
 *
 * `clsx` for the conditionals, `tailwind-merge` so a caller's `className` can
 * override a variant's utility (`p-0` over a row's `px-6 py-5`) instead of
 * both landing in the list and leaving the winner to stylesheet order.
 *
 * tailwind-merge has to be told about the `@theme` names in `src/index.css`.
 * It cannot see the stylesheet, and without this it reads `text-body` as a
 * text *colour* — so `text-body text-fg-secondary` would silently lose one.
 */

import { type ClassValue, clsx } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

const merge = extendTailwindMerge({
  extend: {
    theme: {
      text: [
        "caption2",
        "caption1",
        "footnote",
        "body",
        "callout",
        "headline",
        "title3",
        "title2",
        "title1",
      ],
      font: ["ui", "mono"],
      leading: ["tight", "normal", "prose"],
      radius: ["window", "panel", "card", "control", "chip", "capsule"],
      shadow: ["raised", "floating", "sheet"],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return merge(clsx(inputs));
}
