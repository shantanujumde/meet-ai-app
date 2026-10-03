/**
 * "Show next meeting in the menu bar" (TUR-77): `app.menu_bar_countdown` in
 * `config.jsonc`.
 *
 * On, the next meeting's countdown sits next to the menu-bar icon from an
 * hour before it starts ("Weekly sync in 12m"). Off by default: the icon
 * alone. macOS only shows it; a Windows or Linux tray has no room for text.
 *
 * The same switch as {@link DockSetting}: `role="switch"` with
 * `aria-checked`, named by its visible label, the knob's side as well as the
 * fill showing the state.
 */

import { useEffect, useId, useState } from "react";
import { menuBarCountdown, setMenuBarCountdown } from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Row, RowLabel } from "./primitives";
import { ErrorState } from "./states";

export const MENU_BAR_COUNTDOWN_LABEL = "Show next meeting in the menu bar";

export function MenuBarCountdownSetting() {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const id = useId();

  useEffect(() => {
    let live = true;
    menuBarCountdown()
      .then((saved) => {
        if (live) setOn(saved);
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
      setOn(await setMenuBarCountdown(next));
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
          name={<label htmlFor={id}>{MENU_BAR_COUNTDOWN_LABEL}</label>}
          detail="From an hour before it starts, like “Weekly sync in 12m”, next to the menu bar icon."
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
