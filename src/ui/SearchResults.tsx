/**
 * Search results, shown in place of the meetings list while the box has text.
 *
 * The query is debounced here, and a response only counts if it is still the
 * latest request — typing faster than the backend answers must not let an old
 * answer overwrite a newer one.
 */

import { Fragment, useEffect, useState } from "react";
import { search } from "@/ipc/client";
import type { SearchHit, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { formatRelativeDate } from "@/lib/format";
import { cardVariants, rowVariants } from "@/ui/primitives";
import { Checking, EmptyState, ErrorState } from "@/ui/states";

const SEARCH_DEBOUNCE_MS = 200;
const MARK_OPEN = "«";
const MARK_CLOSE = "»";

type Outcome =
  | { state: "loading" }
  | { state: "done"; hits: SearchHit[] }
  | { state: "error"; error: UiError };

/** Split a snippet on « » so the matched parts can be wrapped in <mark>. */
function renderSnippet(snippet: string) {
  const parts: { text: string; match: boolean }[] = [];
  let rest = snippet;
  while (rest.length > 0) {
    const open = rest.indexOf(MARK_OPEN);
    if (open === -1) {
      parts.push({ text: rest, match: false });
      break;
    }
    if (open > 0) parts.push({ text: rest.slice(0, open), match: false });
    const close = rest.indexOf(MARK_CLOSE, open + 1);
    if (close === -1) {
      parts.push({ text: rest.slice(open + 1), match: false });
      break;
    }
    parts.push({ text: rest.slice(open + 1, close), match: true });
    rest = rest.slice(close + 1);
  }
  return parts.map((part, index) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: a static split of one string, never reordered
    <Fragment key={index}>{part.match ? <mark>{part.text}</mark> : part.text}</Fragment>
  ));
}

export function SearchResults({
  query,
  onOpen,
}: {
  query: string;
  onOpen: (meetingId: string) => void;
}) {
  const [outcome, setOutcome] = useState<Outcome>({ state: "loading" });
  const trimmed = query.trim();

  useEffect(() => {
    let current = true;
    setOutcome({ state: "loading" });
    const timer = window.setTimeout(() => {
      search(trimmed).then(
        (hits) => {
          if (current) setOutcome({ state: "done", hits });
        },
        (failure) => {
          if (current) setOutcome({ state: "error", error: toUiError(failure) });
        },
      );
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [trimmed]);

  if (outcome.state === "loading") return <Checking label="Searching…" />;
  if (outcome.state === "error") return <ErrorState error={outcome.error} />;
  if (outcome.hits.length === 0) {
    return (
      <EmptyState title="No matches" body={`Nothing in your meetings matches “${trimmed}”.`} />
    );
  }

  return (
    <section className={cardVariants({ flush: true })} aria-label="Search results">
      <ul className="m-0 list-none p-0">
        {outcome.hits.map((hit, index) => (
          // One meeting can match on many lines, so the id alone is not unique.
          // biome-ignore lint/suspicious/noArrayIndexKey: results are replaced wholesale
          <li key={`${hit.meetingId}-${index}`}>
            <button
              type="button"
              className={cn(rowVariants({ divided: false }), "w-full flex-col text-start")}
              onClick={() => onOpen(hit.meetingId)}
            >
              <span className="text-body font-medium text-fg-primary">{hit.title}</span>
              <span className="text-caption1 text-fg-tertiary">
                {formatRelativeDate(hit.date)}
                {hit.timestamp ? ` · ${hit.timestamp}` : ""}
              </span>
              <span className="text-footnote text-fg-secondary">{renderSnippet(hit.snippet)}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
