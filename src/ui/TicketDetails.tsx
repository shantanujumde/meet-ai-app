/**
 * A ticket's description, owner and due date, under its title (TUR-113).
 *
 * The description shows its first two lines; pressing it shows the rest. A
 * ticket with no description gets a quiet "No description" badge: it is
 * worth noticing, not an error.
 */

import { useState } from "react";
import type { TicketSummary } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Pill } from "./primitives";

export function TicketDetails({
  ticket,
  className,
}: {
  ticket: TicketSummary;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const description = ticket.body.trim();

  return (
    <div className={cn("flex min-w-0 flex-col gap-2", className)}>
      {description ? (
        <button
          type="button"
          className="text-left"
          aria-expanded={open}
          onClick={() => setOpen((was) => !was)}
        >
          <p
            className={cn(
              "wrap-anywhere whitespace-pre-line text-callout text-fg-secondary",
              open ? null : "line-clamp-2",
            )}
          >
            {description}
          </p>
        </button>
      ) : (
        <span>
          <Pill>No description</Pill>
        </span>
      )}
      {ticket.owner || ticket.due ? (
        <p className="flex flex-wrap gap-x-5 gap-y-1 text-footnote text-fg-secondary">
          {ticket.owner ? <span>Owner: {ticket.owner}</span> : null}
          {ticket.due ? <span>Due: {ticket.due}</span> : null}
        </p>
      ) : null}
    </div>
  );
}
