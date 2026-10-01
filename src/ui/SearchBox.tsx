/** The search field above the meetings list. Controlled: the screen owns the text. */

export function SearchBox({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <input
      type="search"
      aria-label="Search meetings"
      placeholder="Search meetings"
      autoComplete="off"
      spellCheck={false}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      className="w-full rounded-control border-[0.5px] border-rim bg-glass-raised px-4 py-2 text-body text-fg-primary placeholder:text-fg-tertiary"
    />
  );
}
