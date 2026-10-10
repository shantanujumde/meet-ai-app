/**
 * The app's on/off switch, and a Settings row built on it that reads one
 * boolean setting on mount and saves it when flipped. Used by
 * {@link DockSetting} (TUR-76), {@link MenuBarCountdownSetting} (TUR-77) and
 * {@link NotesSwitch}.
 *
 * A real switch: a button with `role="switch"` and `aria-checked`, named by
 * its visible label (a `<label htmlFor>` for a button, so clicking the words
 * flips it too). On and off differ in the knob's side as well as the fill,
 * never by colour alone. TUR-102: bigger (44 × 24, the `--switch-*` tokens),
 * and the "on" fill is the system-blue accent, the way the OS draws its own
 * switches; off is the plain grey control fill.
 */

import { type ReactNode, useId } from "react";
import { useSavedSetting } from "@/hooks/useIpcValue";
import { cn } from "@/lib/cn";
import type { LucideIcon } from "./icons";
import { Row, RowLabel } from "./primitives";
import { ErrorState } from "./states";

export function Switch({
  id,
  on,
  disabled,
  describedBy,
  onChange,
}: {
  id: string;
  on: boolean;
  disabled: boolean;
  /** The id of the helper text, when it should be read with the name. */
  describedBy?: string;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={on}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={cn(
        "inline-flex h-(--switch-h) w-(--switch-w) shrink-0 cursor-default items-center rounded-capsule p-[calc((var(--switch-h)-var(--switch-knob))/2)]",
        "border-[0.5px] border-transparent [transition:background-color_var(--dur-fast)_var(--ease-out)]",
        "disabled:cursor-not-allowed disabled:opacity-40",
        "contrast-more:border contrast-more:border-separator-strong",
        on ? "bg-accent" : "bg-control-hover",
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          "size-(--switch-knob) rounded-capsule bg-on-accent shadow-raised",
          "[transition:translate_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
          on ? "translate-x-[calc(var(--switch-w)-var(--switch-h))]" : "translate-x-0",
        )}
      />
    </button>
  );
}

/**
 * One boolean setting as a Settings row. `load` reads the saved value; `save`
 * writes a new one and returns what was saved, which is what the switch then
 * shows. Until `load` answers the switch is off and disabled; a failed save
 * keeps the old state and shows the error under the row.
 */
export function SettingSwitch({
  label,
  detail,
  icon,
  load,
  save,
}: {
  label: string;
  detail: ReactNode;
  /** The row's icon, in its rounded square (TUR-102). */
  icon?: LucideIcon;
  /** Read on mount; pass a function defined outside the component. */
  load: () => Promise<boolean>;
  save: (on: boolean) => Promise<boolean>;
}) {
  const { value: on, busy, error, change } = useSavedSetting(load, save);
  const id = useId();

  return (
    <>
      <Row>
        <RowLabel
          icon={icon}
          name={<label htmlFor={id}>{label}</label>}
          detail={detail}
          mono={false}
        />
        <Switch
          id={id}
          on={on ?? false}
          disabled={busy || on === null}
          onChange={(next) => void change(next)}
        />
      </Row>
      {error ? <ErrorState error={error} /> : null}
    </>
  );
}
