/**
 * The Light / Dark / System picker (TUR-102): three picture tiles, each a
 * tiny drawing of a window in that look. System is split corner to corner,
 * light over dark.
 *
 * A real radio group: native radios, visually hidden, inside labels that
 * wrap each whole tile, so a click anywhere on the tile picks it and the
 * arrow keys move between the three, both for free. The chosen tile shows
 * an accent outline, a faint accent fill and its name in the accent, never
 * the colour alone: the outline is a shape too.
 */

import { useId } from "react";
import type { ThemeChoice } from "@/ipc/client";
import { cn } from "@/lib/cn";

const CHOICES: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function AppearanceTiles({
  value,
  disabled = false,
  onChange,
  labelledBy,
}: {
  value: ThemeChoice;
  disabled?: boolean;
  onChange: (value: ThemeChoice) => void;
  /** The id of the text that names the group. */
  labelledBy?: string;
}) {
  const name = useId();
  return (
    <div role="radiogroup" aria-labelledby={labelledBy} className="flex flex-wrap gap-5">
      {CHOICES.map((choice) => {
        const checked = choice.value === value;
        return (
          <label
            key={choice.value}
            className={cn(
              "group flex flex-col items-center gap-3 rounded-(--card-radius) p-3",
              "has-[:focus-visible]:outline-solid has-[:focus-visible]:outline-(length:--focus-w)",
              "has-[:focus-visible]:outline-(color:--focus-ring) has-[:focus-visible]:outline-offset-(--focus-offset)",
              checked ? "bg-accent-glass" : "",
              disabled ? "cursor-not-allowed opacity-40" : "cursor-pointer",
            )}
          >
            <input
              type="radio"
              name={name}
              value={choice.value}
              checked={checked}
              disabled={disabled}
              onChange={() => onChange(choice.value)}
              className="sr-only"
            />
            <span
              aria-hidden="true"
              className={cn(
                "relative block h-(--tile-h) w-(--tile-w) overflow-hidden rounded-control",
                "border-[0.5px] border-separator-strong shadow-raised",
                checked ? "outline-2 outline-offset-1 outline-accent outline-solid" : "",
              )}
            >
              {choice.value === "dark" ? (
                <WindowDrawing look="dark" />
              ) : (
                <WindowDrawing look="light" />
              )}
              {choice.value === "system" ? (
                // The dark half, cut corner to corner over the light one.
                <span className="absolute inset-0 [clip-path:polygon(100%_0,100%_100%,0_100%)]">
                  <WindowDrawing look="dark" />
                </span>
              ) : null}
            </span>
            <span
              className={cn(
                "text-footnote font-semibold",
                checked ? "text-accent-text" : "text-fg-primary",
              )}
            >
              {choice.label}
            </span>
          </label>
        );
      })}
    </div>
  );
}

const LOOK = {
  light: {
    window: "bg-(--preview-light-window)",
    sidebar: "bg-(--preview-light-sidebar)",
    bar: "bg-(--preview-light-bar)",
    line: "bg-(--preview-light-line)",
  },
  dark: {
    window: "bg-(--preview-dark-window)",
    sidebar: "bg-(--preview-dark-sidebar)",
    bar: "bg-(--preview-dark-bar)",
    line: "bg-(--preview-dark-line)",
  },
} as const;

/** A window in miniature: a title bar with its three dots, a sidebar, lines of text. */
function WindowDrawing({ look }: { look: keyof typeof LOOK }) {
  const colours = LOOK[look];
  return (
    <span className={cn("absolute inset-0 flex flex-col", colours.window)}>
      <span className={cn("flex h-5 shrink-0 items-center gap-1 px-3", colours.bar)}>
        <span className="size-[5px] rounded-capsule bg-(--preview-dot-close)" />
        <span className="size-[5px] rounded-capsule bg-(--preview-dot-min)" />
        <span className="size-[5px] rounded-capsule bg-(--preview-dot-max)" />
      </span>
      <span className="flex min-h-0 flex-1">
        <span className={cn("flex w-[34%] flex-col gap-2 p-3", colours.sidebar)}>
          <span className={cn("h-[3px] w-4/5 rounded-capsule", colours.line)} />
          <span className={cn("h-[3px] w-3/5 rounded-capsule", colours.line)} />
          <span className={cn("h-[3px] w-2/3 rounded-capsule", colours.line)} />
        </span>
        <span className="flex flex-1 flex-col gap-2 p-4">
          <span className={cn("h-[4px] w-3/5 rounded-capsule", colours.line)} />
          <span className={cn("h-[3px] w-4/5 rounded-capsule", colours.line)} />
          <span className={cn("h-[3px] w-2/3 rounded-capsule", colours.line)} />
        </span>
      </span>
    </span>
  );
}
