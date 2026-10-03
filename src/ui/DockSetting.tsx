/**
 * "Show in Dock when the window is closed" (TUR-76): the `app` section's one
 * switch, `app.show_in_dock_when_closed` in `config.jsonc`.
 *
 * Closing the window never quits meet-ai: it keeps running in the menu bar
 * so reminders, detection and a recording carry on. Off (the default), the
 * Dock icon goes away with the window, like Granola; on, it stays.
 *
 * A real switch, like the notes switch: `role="switch"` with `aria-checked`,
 * named by its visible label, the knob's side as well as the fill showing
 * the state.
 */

import { useEffect, useId, useState } from "react";
import { appSettings, setShowInDockWhenClosed } from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Row, RowLabel } from "./primitives";
import { ErrorState } from "./states";

export const DOCK_SETTING_LABEL = "Show in Dock when the window is closed";

export function DockSetting() {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const id = useId();

  useEffect(() => {
    let live = true;
    appSettings()
      .then((settings) => {
        if (live) setOn(settings.showInDockWhenClosed);
      })
      .catch((thrown: unknown) => {
        if (live) setError(toUiError(thrown));
      });
    return () => {
      live = false;
    };
  }, []);

  const change = async (next: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const saved = await setShowInDockWhenClosed(next);
      setOn(saved.showInDockWhenClosed);
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  };

  const checked = on ?? false;

  return (
    <>
      <Row>
        <RowLabel
          name={<label htmlFor={id}>{DOCK_SETTING_LABEL}</label>}
          detail="Closing the window keeps meet-ai running in the menu bar. Quit from the menu bar icon or with ⌘Q."
          mono={false}
        />
        <button
          type="button"
          role="switch"
          id={id}
          aria-checked={checked}
          disabled={busy || on === null}
          onClick={() => void change(!checked)}
          className={cn(
            "inline-flex h-(--control-h-small) w-[36px] shrink-0 cursor-default items-center rounded-capsule p-1",
            "border-[0.5px] [transition:background-color_var(--dur-fast)_var(--ease-out)]",
            "disabled:cursor-not-allowed disabled:opacity-40",
            "contrast-more:border contrast-more:border-separator-strong",
            checked ? "border-transparent bg-success" : "border-rim bg-glass-sunken",
          )}
        >
          <span
            aria-hidden="true"
            className={cn(
              "size-6 rounded-capsule bg-on-accent shadow-raised",
              "[transition:translate_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
              checked ? "translate-x-6" : "translate-x-0",
            )}
          />
        </button>
      </Row>
      {error ? <ErrorState error={error} /> : null}
    </>
  );
}
