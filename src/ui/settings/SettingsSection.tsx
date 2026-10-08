/**
 * A settings group and its rows (TUR-102): the shape every Settings section
 * and every card-of-rows screen shares, so no screen keeps its own copy.
 *
 * {@link SettingsSection} is a heading over one rounded card. A section is a
 * named region (`aria-labelledby` its heading), which is how tests and
 * VoiceOver find "the Audio section".
 *
 * {@link SettingsRow} is the one row pattern: an icon in a small rounded
 * square, the name in bold, one grey line explaining it, and the control on
 * the right. Thin hairlines between rows in the same card come from `Row`.
 */

import { type ReactNode, useId } from "react";
import { cn } from "@/lib/cn";
import type { LucideIcon } from "../icons";
import { Card, Row, RowLabel } from "../primitives";

export function SettingsSection({
  title,
  description,
  children,
  after,
  anchorId,
  className,
}: {
  /** The section's element id, for `/settings?section=<id>` to scroll to (TUR-113). */
  anchorId?: string;
  title: ReactNode;
  /** One quiet sentence under the heading, when the section needs it. */
  description?: ReactNode;
  /** The card's rows. */
  children: ReactNode;
  /** Anything below the card that is not a row (an error, a note). */
  after?: ReactNode;
  className?: string;
}) {
  const id = useId();
  return (
    <section
      id={anchorId}
      className={cn("flex scroll-mt-6 flex-col gap-(--section-gap)", className)}
      aria-labelledby={id}
    >
      <header className="flex flex-col gap-1 px-1">
        <h2 className="m-0 text-headline font-semibold" id={id}>
          {title}
        </h2>
        {description ? (
          <p className="m-0 text-footnote text-fg-secondary contrast-more:text-fg-primary">
            {description}
          </p>
        ) : null}
      </header>
      <Card flush>{children}</Card>
      {after}
    </section>
  );
}

/** A `<select>` in a settings row's control slot. */
export const SETTINGS_SELECT =
  "rounded-control border-[0.5px] border-separator bg-glass-sunken px-3 py-2 text-footnote text-fg-primary disabled:cursor-not-allowed disabled:opacity-40";

export function SettingsRow({
  icon,
  name,
  detail,
  control,
  children,
  mono = false,
  className,
}: {
  icon?: LucideIcon;
  name: ReactNode;
  /** The grey line: what the setting does. */
  detail?: ReactNode;
  /** The switch, picker or button on the right. */
  control?: ReactNode;
  /** More under the row's line (progress, a note, an error). */
  children?: ReactNode;
  /** The grey line in monospace, for a path. */
  mono?: boolean;
  className?: string;
}) {
  const line = (
    <>
      <RowLabel icon={icon} name={name} detail={detail} mono={mono} />
      {control !== undefined ? (
        <span className="flex shrink-0 items-center gap-4">{control}</span>
      ) : null}
    </>
  );
  if (children === undefined) return <Row className={className}>{line}</Row>;
  return (
    <Row stacked className={cn("gap-5", className)}>
      <span className="flex items-center justify-between gap-6">{line}</span>
      {children}
    </Row>
  );
}
