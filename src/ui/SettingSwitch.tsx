/**
 * The app's on/off switch, and a Settings row built on it that reads one
 * boolean setting on mount and saves it when flipped. Used by
 * {@link DockSetting} (TUR-76), {@link MenuBarCountdownSetting} (TUR-77) and
 * {@link NotesSwitch}.
 *
 * A real switch: a button with `role="switch"` and `aria-checked`, named by
 * its visible label (a `<label htmlFor>` for a button, so clicking the words
 * flips it too). On and off differ in the knob's side as well as the fill,
 * never by colour alone. The "on" fill is green rather than the accent: the
 * accent is kept for the one primary control per window (Record, or Stop).
 */

import { type ReactNode, useEffect, useId, useState } from "react";
import { toUiError, type UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
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
        "inline-flex h-(--control-h-small) w-[36px] shrink-0 cursor-default items-center rounded-capsule p-1",
        "border-[0.5px] [transition:background-color_var(--dur-fast)_var(--ease-out)]",
        "disabled:cursor-not-allowed disabled:opacity-40",
        "contrast-more:border contrast-more:border-separator-strong",
        on ? "border-transparent bg-success" : "border-rim bg-glass-sunken",
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          "size-6 rounded-capsule bg-on-accent shadow-raised",
          "[transition:translate_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
          on ? "translate-x-6" : "translate-x-0",
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
  load,
  save,
}: {
  label: string;
  detail: ReactNode;
  /** Read on mount; pass a function defined outside the component. */
  load: () => Promise<boolean>;
  save: (on: boolean) => Promise<boolean>;
}) {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const id = useId();

  useEffect(() => {
    let live = true;
    load()
      .then((saved) => {
        if (live) setOn(saved);
      })
      .catch((thrown: unknown) => {
        if (live) setError(toUiError(thrown));
      });
    return () => {
      live = false;
    };
  }, [load]);

  const change = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setOn(await save(next));
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Row>
        <RowLabel name={<label htmlFor={id}>{label}</label>} detail={detail} mono={false} />
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
