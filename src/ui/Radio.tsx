/**
 * One choice in a radio group: a native `<input type="radio">` inside a
 * `<label>` that wraps the whole choice, so a click anywhere on the label
 * picks it and the arrow keys move through the group, both for free.
 *
 * The input keeps its role and keyboard behaviour; only its look is redrawn
 * (`appearance: none`), because the WebKit default is drawn at whatever the
 * box is, and `size-4` here is 8 px — the 4pt grid, not Tailwind's rem steps
 * (TUR-73). 16 px across, a 1.5 px ring when off, an accent disc with a white
 * centre when on. The tokens carry dark mode and Increase Contrast.
 *
 * The children are the label: the option's name first, then anything under
 * it. The radio sits on the first line of the children, not the middle of
 * the block.
 */

import { cva } from "class-variance-authority";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

export const radioLabelVariants = cva("group flex min-w-0 items-start gap-4", {
  variants: {
    disabled: {
      true: "cursor-not-allowed opacity-40",
      false: "cursor-pointer",
    },
  },
  defaultVariants: { disabled: false },
});

/**
 * The control itself. The ring is `fg-secondary`, not a separator: a
 * separator is 10–20% alpha and an unchecked ring needs 3:1 against the row
 * to be seen. Checked turns the border into a 5 px accent band around a 6 px
 * `on-accent` centre, so it needs no pseudo-element and no gradient. The
 * focus ring is the app-wide `:focus-visible` outline; its 8 px radius is a
 * circle on a 16 px box.
 */
export const radioInputVariants = cva(
  [
    "m-0 size-6 shrink-0 appearance-none rounded-full",
    "border-[1.5px] border-solid border-fg-secondary bg-transparent",
    "transition-[border-color,border-width] duration-100 ease-out motion-reduce:transition-none",
    "checked:border-[5px] checked:border-accent checked:bg-on-accent",
    "contrast-more:border-fg-primary",
  ],
  {
    variants: {
      disabled: {
        true: "cursor-not-allowed",
        false: "cursor-pointer group-hover:border-fg-primary group-hover:checked:border-accent",
      },
    },
    defaultVariants: { disabled: false },
  },
);

export function Radio({
  name,
  value,
  checked,
  disabled = false,
  onChange,
  className,
  children,
}: {
  /** The group's shared `name`; arrow keys move between radios that share it. */
  name: string;
  value: string;
  checked: boolean;
  disabled?: boolean;
  /** Called with this radio's `value` when it is picked. */
  onChange: (value: string) => void;
  /** Added to the label, such as `flex-1` to fill a row. */
  className?: string;
  /** The label: the name, then any detail under it. */
  children: ReactNode;
}) {
  return (
    <label className={cn(radioLabelVariants({ disabled }), className)}>
      {/* One line of the label's own text tall, so the radio centres on the
          first line however many lines follow. */}
      <span className="flex h-[1lh] shrink-0 items-center text-body">
        <input
          type="radio"
          name={name}
          value={value}
          checked={checked}
          disabled={disabled}
          onChange={() => onChange(value)}
          className={radioInputVariants({ disabled })}
        />
      </span>
      <span className="flex min-w-0 flex-col gap-1">{children}</span>
    </label>
  );
}
