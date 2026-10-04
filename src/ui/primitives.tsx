/**
 * The small, repeated pieces every screen is built from — buttons, pills,
 * cards and their rows — as Tailwind utilities with `cva` variants.
 *
 * TUR-102 restyled them after a calm, roomy settings window: rounded cards a
 * step off the brand-tinted canvas, one row pattern (an icon in a small
 * rounded square, the name in bold, one grey line, the control on the
 * right), plain grey secondary buttons, and Lucide icons through `./icons`.
 * Every utility resolves to a design-system token (the `@theme` block in
 * `src/theme.css`). Rules from MASTER.md that are easy to lose, restated
 * where they apply:
 *
 * * One tinted control per window: `tone: "primary"`. Everything else is
 *   neutral grey.
 * * Increase Contrast turns soft edges into solid 1px borders and secondary
 *   text into primary — glass is decoration and the interface survives
 *   losing it.
 * * Rows carry no radius: a flush card's `overflow-hidden` clips their corners.
 *
 * The variant functions are exported as well as the components, for elements
 * that need the look without the element — a `NavLink` styled as a button, a
 * whole meeting row that is itself a button.
 */

import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/cn";
import { Icon, IconSquare, type LucideIcon } from "./icons";

// --- buttons --------------------------------------------------------------

export const buttonVariants = cva(
  [
    "inline-flex items-center justify-center gap-3 whitespace-nowrap",
    "cursor-default rounded-control border-[0.5px] font-medium",
    // `scale`, not `transform`: Tailwind's scale utility is the standalone
    // property, so pressing a positioned button (the live pane's "New lines
    // below") no longer drops its `translate`.
    "[transition:background-color_var(--dur-fast)_var(--ease-out),scale_var(--dur-instant)_var(--ease-out)]",
    "not-disabled:active:scale-97 disabled:cursor-not-allowed disabled:opacity-40",
    "contrast-more:border contrast-more:border-separator-strong",
  ],
  {
    variants: {
      tone: {
        // Plain grey: every button but the one primary action (TUR-102).
        neutral:
          "border-transparent bg-control text-fg-primary not-disabled:hover:bg-control-hover",
        // `accent-fill`, not the raw accent: white on it holds 4.5:1.
        primary:
          "border-transparent bg-accent-fill text-on-accent not-disabled:hover:bg-accent-hover",
        quiet:
          "border-transparent bg-transparent text-fg-secondary not-disabled:hover:bg-control not-disabled:hover:text-fg-primary",
      },
      size: {
        regular: "h-(--control-h-large) px-5 text-body",
        small: "h-(--control-h-regular) px-4 text-footnote",
      },
      block: {
        true: "w-full",
      },
    },
    defaultVariants: { tone: "neutral", size: "regular" },
  },
);

type ButtonVariants = VariantProps<typeof buttonVariants>;

export function Button({
  tone,
  size,
  block,
  icon,
  className,
  type = "button",
  children,
  ...props
}: ComponentProps<"button"> &
  ButtonVariants & {
    /** A Lucide icon before the words. */
    icon?: LucideIcon;
  }) {
  return (
    <button type={type} className={cn(buttonVariants({ tone, size, block }), className)} {...props}>
      {icon ? <Icon icon={icon} /> : null}
      {children}
    </button>
  );
}

/**
 * A button that is only an icon. `label` is required: it is the button's
 * accessible name and its tooltip, because an icon alone names nothing to
 * VoiceOver (MASTER.md §5.2).
 */
export function IconButton({
  icon,
  label,
  tone = "quiet",
  className,
  type = "button",
  ...props
}: Omit<ComponentProps<"button">, "children" | "aria-label"> & {
  icon: LucideIcon;
  label: string;
  tone?: ButtonVariants["tone"];
}) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      className={cn(
        buttonVariants({ tone, size: "small" }),
        "size-(--control-h-large) px-0",
        className,
      )}
      {...props}
    >
      <Icon icon={icon} />
    </button>
  );
}

/** A wrapping row of buttons, and whatever small text sits beside them. */
export function ButtonRow({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex flex-wrap items-center gap-4", className)} {...props} />;
}

// --- pills ----------------------------------------------------------------

/** A short uppercase status word. The word carries the meaning; the tone only matches it. */
export const pillVariants = cva(
  [
    "inline-flex h-(--control-h-small) items-center gap-2 whitespace-nowrap px-4",
    "rounded-capsule bg-glass-sunken text-caption2 font-medium uppercase tracking-[0.04em]",
  ],
  {
    variants: {
      tone: {
        neutral: "text-fg-secondary",
        ok: "text-success",
        warn: "text-warning",
        danger: "text-danger",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export function Pill({
  tone,
  className,
  ...props
}: ComponentProps<"span"> & VariantProps<typeof pillVariants>) {
  return <span className={cn(pillVariants({ tone }), className)} {...props} />;
}

// --- cards and rows -------------------------------------------------------

export const cardVariants = cva(
  [
    // Opaque, a step off the canvas, no outline: the fill alone separates it
    // (TUR-102). Increase Contrast adds a solid edge.
    "flex flex-col gap-5 rounded-(--card-radius) bg-card",
    "contrast-more:border contrast-more:border-separator-strong",
  ],
  {
    variants: {
      /**
       * Rows sit edge to edge inside the card's side padding, so the
       * hairlines between them stop short of the card's edges.
       */
      flush: {
        true: "gap-0 overflow-hidden px-(--card-pad-x)",
        false: "p-6",
      },
    },
    defaultVariants: { flush: false },
  },
);

export function Card({
  flush,
  className,
  ...props
}: ComponentProps<"div"> & VariantProps<typeof cardVariants>) {
  return <div className={cn(cardVariants({ flush }), className)} {...props} />;
}

export const rowVariants = cva("flex justify-between gap-6", {
  variants: {
    /** Label and control on one line, with more (progress, an error) below. */
    stacked: {
      true: "flex-col items-stretch",
      false: "items-center",
    },
    /**
     * Inside a padded card, whose own padding already frames the row. Not
     * bare, the row pads itself top and bottom; a flush card pads the sides.
     */
    bare: {
      true: "p-0",
      false: "min-h-(--row-min-h) py-(--row-pad-y)",
    },
    /** A hairline between consecutive rows. */
    divided: {
      true: "[&+&]:border-t-[0.5px] [&+&]:border-separator",
      false: "",
    },
  },
  defaultVariants: { stacked: false, bare: false, divided: true },
});

export function Row({
  stacked,
  bare,
  divided,
  className,
  ...props
}: ComponentProps<"div"> & VariantProps<typeof rowVariants>) {
  return <div className={cn(rowVariants({ stacked, bare, divided }), className)} {...props} />;
}

/**
 * The grey line under a row's name. Monospace by default, for paths and
 * other evidence; `mono: false` for a sentence.
 */
export const rowDetailVariants = cva(
  "text-caption1 leading-normal text-fg-secondary wrap-anywhere contrast-more:text-fg-primary",
  {
    variants: {
      mono: {
        true: "font-mono",
        false: "font-ui",
      },
    },
    defaultVariants: { mono: true },
  },
);

/**
 * A row's left side, the one pattern every settings row follows (TUR-102):
 * an icon in a small rounded square, the name in bold, and one grey line
 * under it saying what the row does.
 */
export function RowLabel({
  name,
  detail,
  mono,
  icon,
}: {
  name: ReactNode;
  detail?: ReactNode;
  mono?: boolean;
  /** The row's Lucide icon, drawn in its rounded square. */
  icon?: LucideIcon;
}) {
  const text = (
    <span className="flex min-w-0 flex-col gap-1">
      <span className="text-body font-semibold">{name}</span>
      {detail !== undefined ? <span className={rowDetailVariants({ mono })}>{detail}</span> : null}
    </span>
  );
  if (!icon) return text;
  return (
    <span className="flex min-w-0 items-center gap-5">
      <IconSquare icon={icon} />
      {text}
    </span>
  );
}

export const rowValueClass =
  "text-right text-footnote tabular-nums text-fg-secondary contrast-more:text-fg-primary";

/** A row's right side, when it is a value rather than a control. */
export function RowValue({ className, ...props }: ComponentProps<"span">) {
  return <span className={cn(rowValueClass, className)} {...props} />;
}

// --- text -----------------------------------------------------------------

/** A paragraph of explanation, at the notes pane's reading measure. */
export function Prose({ className, ...props }: ComponentProps<"p">) {
  return (
    <p
      className={cn(
        "max-w-(--notes-measure) text-callout leading-prose text-fg-secondary contrast-more:text-fg-primary",
        className,
      )}
      {...props}
    />
  );
}
