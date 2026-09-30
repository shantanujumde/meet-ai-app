/**
 * The small, repeated pieces every screen is built from — buttons, pills,
 * cards and their rows — as Tailwind utilities with `cva` variants.
 *
 * These replace the `.btn`, `.badge`, `.card` and `.row` blocks that used to
 * live in `app.css`, value for value: every utility here resolves to the same
 * design-system token the old rule used (see the `@theme` block in
 * `src/index.css`). Rules from MASTER.md that are easy to lose in a move, and
 * so are restated where they apply:
 *
 * * One tinted control per window: `tone: "primary"`. Everything else is
 *   neutral.
 * * Increase Contrast turns rims into solid 1px borders and secondary text
 *   into primary — glass is decoration and the interface survives losing it.
 * * Rows carry no radius: a flush card's `overflow-hidden` clips their corners.
 *   The concentric rule would compute `--radius-panel - --space-6` for them,
 *   which is negative — the sign that those corners are the card's.
 *
 * The variant functions are exported as well as the components, for elements
 * that need the look without the element — a `NavLink` styled as a button, a
 * whole meeting row that is itself a button.
 */

import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/cn";

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
        neutral: "border-rim bg-glass-raised text-fg-primary not-disabled:hover:bg-glass-regular",
        primary: "border-transparent bg-accent text-on-accent not-disabled:hover:bg-accent-hover",
        quiet:
          "border-transparent bg-transparent text-fg-secondary not-disabled:hover:bg-row-hover not-disabled:hover:text-fg-primary",
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
  className,
  type = "button",
  ...props
}: ComponentProps<"button"> & ButtonVariants) {
  return (
    <button
      type={type}
      className={cn(buttonVariants({ tone, size, block }), className)}
      {...props}
    />
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
    "flex flex-col gap-5 rounded-panel border-[0.5px] border-separator bg-content-alt",
    "contrast-more:border contrast-more:border-separator-strong",
  ],
  {
    variants: {
      /** Rows sit edge to edge and pad themselves; the card only clips them. */
      flush: {
        true: "overflow-hidden",
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

export const rowVariants = cva("flex justify-between gap-5", {
  variants: {
    /** Label and control on one line, with more (progress, an error) below. */
    stacked: {
      true: "flex-col items-stretch",
      false: "items-center",
    },
    /** Inside a padded card, whose own padding already frames the row. */
    bare: {
      true: "p-0",
      false: "px-6 py-5",
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
 * The quiet line under a row's name. Monospace by default, for paths and
 * other evidence; `mono: false` for a sentence.
 */
export const rowDetailVariants = cva("text-caption1 text-fg-tertiary wrap-anywhere", {
  variants: {
    mono: {
      true: "font-mono",
      false: "font-ui",
    },
  },
  defaultVariants: { mono: true },
});

/** A row's left side: its name, and a detail line under it. */
export function RowLabel({
  name,
  detail,
  mono,
}: {
  name: ReactNode;
  detail?: ReactNode;
  mono?: boolean;
}) {
  return (
    <span className="flex min-w-0 flex-col gap-1">
      <span className="text-body font-medium">{name}</span>
      {detail !== undefined ? <span className={rowDetailVariants({ mono })}>{detail}</span> : null}
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
