/** The search field above the meetings list. Controlled: the screen owns the text. */

import { Search } from "lucide-react";
import { Icon } from "./icons";

export function SearchBox({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    // The focus ring goes round the whole rounded box, not the bare input in it.
    <div className="flex items-center gap-4 rounded-control border-[0.5px] border-separator bg-control px-5 text-fg-secondary has-[:focus-visible]:outline-solid has-[:focus-visible]:outline-(length:--focus-w) has-[:focus-visible]:outline-(color:--focus-ring) has-[:focus-visible]:outline-offset-(--focus-offset) contrast-more:border-separator-strong">
      <Icon icon={Search} />
      <input
        type="search"
        aria-label="Search meetings"
        placeholder="Search meetings"
        autoComplete="off"
        spellCheck={false}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-(--control-h-large) w-full bg-transparent text-body text-fg-primary placeholder:text-fg-tertiary focus-visible:outline-none"
      />
    </div>
  );
}
