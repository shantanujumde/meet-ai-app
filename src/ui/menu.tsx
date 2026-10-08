/**
 * The in-window menu (TUR-116): one list of actions, opened two ways from
 * the same row. A ⋯ button that appears on hover, on keyboard focus and on
 * the selected row, and a right-click (Control-click on a Mac) anywhere on
 * the row, which replaces the system's text menu there. Both draw the same
 * items in the same groups, from one {@link MenuGroups} value.
 *
 * Built on Radix's DropdownMenu and ContextMenu, which bring the keyboard
 * rules a menu needs: arrow keys move, Enter or Space picks, Escape closes
 * and puts focus back. The context-menu key and Shift+F10 on the row open
 * the ⋯ menu, since a keyboard has no mouse position to open one at.
 *
 * Styled as the design system's popover (MASTER.md §5.5): near-solid popup
 * fill, the panel radius, items at the popover item height with a
 * concentric radius, and the highlighted item in the accent. A destructive
 * item is red, and red when highlighted.
 */

import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Ellipsis } from "lucide-react";
import { type ComponentType, Fragment, type KeyboardEvent, type ReactNode, useState } from "react";
import { cn } from "@/lib/cn";
import { Icon } from "./icons";

/** One thing a menu can do. */
export type MenuItem = {
  /** Unique within its menu; also the React key. */
  key: string;
  label: string;
  /** Deletes or otherwise loses something: drawn in the danger colour. */
  danger?: boolean;
  onSelect: () => void;
};

/** The items in groups, with a divider between each. Empty groups are skipped. */
export type MenuGroups = readonly (readonly MenuItem[])[];

const contentClass = cn(
  "z-(--z-popover) min-w-[200px] max-w-[320px] overflow-hidden",
  "rounded-(--popover-radius) p-(--popover-pad)",
  "border-[0.5px] border-separator bg-popup shadow-floating",
  "font-ui text-body text-fg-primary outline-none",
  "contrast-more:border-separator-strong",
);

const itemClass = (danger: boolean | undefined) =>
  cn(
    "flex h-(--popover-item-h) cursor-default select-none items-center",
    "rounded-(--popover-item-radius) px-4 outline-none",
    danger
      ? "text-danger data-highlighted:bg-danger-fill data-highlighted:text-on-accent"
      : "data-highlighted:bg-accent-fill data-highlighted:text-on-accent",
  );

const separatorClass = "mx-2 my-2 h-[0.5px] bg-separator contrast-more:bg-separator-strong";

/** The parts both Radix menus share the shape of. */
type Parts = {
  Item: ComponentType<{ className?: string; onSelect?: () => void; children?: ReactNode }>;
  Separator: ComponentType<{ className?: string }>;
  Group: ComponentType<{ children?: ReactNode }>;
};

const DROPDOWN: Parts = {
  Item: DropdownMenu.Item,
  Separator: DropdownMenu.Separator,
  Group: DropdownMenu.Group,
};
const CONTEXT: Parts = {
  Item: ContextMenu.Item,
  Separator: ContextMenu.Separator,
  Group: ContextMenu.Group,
};

/** The items, the same for both menus. */
function Items({
  groups,
  parts: { Item, Separator, Group },
}: {
  groups: MenuGroups;
  parts: Parts;
}) {
  const shown = groups.filter((group) => group.length > 0);
  return shown.map((group, index) => (
    <Fragment key={group[0]?.key ?? index}>
      {index > 0 ? <Separator className={separatorClass} /> : null}
      <Group>
        {group.map((item) => (
          <Item key={item.key} className={itemClass(item.danger)} onSelect={item.onSelect}>
            <span className="truncate">{item.label}</span>
          </Item>
        ))}
      </Group>
    </Fragment>
  ));
}

/** The keys a keyboard opens a context menu with. */
function isMenuKey(event: KeyboardEvent): boolean {
  return event.key === "ContextMenu" || (event.shiftKey && event.key === "F10");
}

/**
 * A row with a menu: `children` is the row itself (its button), and the ⋯
 * button sits beside it, not inside it, at the row's end. The wrapper is
 * `relative` for that button and a named `group` so it can show on hover.
 */
export function MenuRow({
  label,
  groups,
  onOpenChange,
  shown = false,
  disabled = false,
  className,
  buttonClassName,
  children,
}: {
  /** The ⋯ button's accessible name and tooltip, e.g. "More actions for Standup". */
  label: string;
  groups: MenuGroups;
  /** Either menu opened or closed: the moment to fetch what the items depend on. */
  onOpenChange?: (open: boolean) => void;
  /** Keep the ⋯ button visible without hover or focus: the selected row. */
  shown?: boolean;
  /** No menu at all: the row is being edited, and the text field keeps its own. */
  disabled?: boolean;
  className?: string;
  /** Colour for the ⋯ button, to match the row it sits on. */
  buttonClassName?: string;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const change = (next: boolean) => {
    setOpen(next);
    onOpenChange?.(next);
  };

  return (
    <ContextMenu.Root onOpenChange={onOpenChange}>
      <ContextMenu.Trigger asChild disabled={disabled}>
        {/* biome-ignore lint/a11y/noStaticElementInteractions: the keys come from the row's own button inside, bubbling up; the wrapper is never focused itself. */}
        <div
          className={cn("group/menu-row relative", className)}
          onKeyDown={(event) => {
            if (disabled || open || !isMenuKey(event)) return;
            event.preventDefault();
            change(true);
          }}
        >
          {children}
          {disabled ? null : (
            <DropdownMenu.Root open={open} onOpenChange={change}>
              <DropdownMenu.Trigger asChild>
                <button
                  type="button"
                  aria-label={label}
                  title={label}
                  className={cn(
                    "absolute end-2 top-1/2 -translate-y-1/2",
                    "inline-flex size-(--control-h-regular) cursor-default items-center justify-center",
                    "rounded-control border-0 bg-transparent text-fg-secondary hover:bg-control",
                    "[transition:opacity_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
                    shown
                      ? "opacity-100"
                      : "opacity-0 focus-visible:opacity-100 group-hover/menu-row:opacity-100 group-focus-within/menu-row:opacity-100 data-[state=open]:opacity-100",
                    buttonClassName,
                  )}
                >
                  <Icon icon={Ellipsis} />
                </button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content align="end" sideOffset={4} className={contentClass}>
                  <Items groups={groups} parts={DROPDOWN} />
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          )}
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={contentClass}>
          <Items groups={groups} parts={CONTEXT} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
