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

/**
 * The small rounded square a settings row's icon sits in: the accent icon on
 * a faint accent fill. Decorative; the row's name says what it is.
 */
export function IconSquare({ icon, className }: { icon: LucideIcon; className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "grid size-(--icon-box) shrink-0 place-items-center rounded-(--icon-box-radius)",
        "bg-accent-glass text-accent-text",
        "contrast-more:border contrast-more:border-separator-strong",
        className,
      )}
    >
      <Icon icon={icon} />
    </span>
  );
}
