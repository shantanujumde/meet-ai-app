/**
 * The app's icons (TUR-102): Lucide, at one size and one stroke weight.
 *
 * Always draw an icon through {@link Icon} (or {@link IconSquare}), never a
 * Lucide component bare: that is what keeps every icon 16 px with the same
 * 1.75 stroke (MASTER.md §5.2, "one family, one weight"). Lucide's own
 * defaults are 24 px at 2.
 *
 * Icons are decoration next to words, so they are hidden from VoiceOver. A
 * control that is only an icon names itself with `aria-label` (see
 * `IconButton` in primitives.tsx); `label` here is for the rare icon that
 * carries meaning on its own.
 */

import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/cn";

/** Every icon's size and stroke, from `--icon-size` and `--icon-stroke`. */
export const ICON_SIZE = 16;
export const ICON_STROKE = 1.75;

export type { LucideIcon };

export function Icon({
  icon: Glyph,
  className,
  label,
}: {
  icon: LucideIcon;
  className?: string;
  /** Only for an icon that means something with no words beside it. */
  label?: string;
}) {
  return (
    <Glyph
      size={ICON_SIZE}
      strokeWidth={ICON_STROKE}
      className={cn("shrink-0", className)}
      {...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true })}
    />
  );
}

const SQUARE_TONE = {
  accent: "bg-accent-glass text-accent-text",
  // The AA shades as the icon, on a faint wash of the same hue.
  danger: "bg-danger/12 text-danger",
  warning: "bg-warning/14 text-warning",
} as const;

/**
 * The small rounded square a settings row's icon sits in: the accent icon on
 * a faint accent fill, or the warning or danger hue for an error or a
 * question with a cost. Decorative; the words beside it say what it is.
 */
export function IconSquare({
  icon,
  tone = "accent",
  className,
}: {
  icon: LucideIcon;
  tone?: keyof typeof SQUARE_TONE;
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "grid size-(--icon-box) shrink-0 place-items-center rounded-(--icon-box-radius)",
        SQUARE_TONE[tone],
        "contrast-more:border contrast-more:border-separator-strong",
        className,
      )}
    >
      <Icon icon={icon} />
    </span>
  );
}
