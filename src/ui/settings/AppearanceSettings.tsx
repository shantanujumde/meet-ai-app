/**
 * Settings → Appearance (TUR-102): Light / Dark / System as picture tiles,
 * and the switch for the see-through glass look. Both are saved in
 * `config.jsonc` (`appearance.theme`, `appearance.glass`) and change the
 * window at once, through the appearance store. A value in config.jsonc
 * Rust could not use is named under the card (TUR-155).
 */

import { Sparkles, SunMoon } from "lucide-react";
import { useId } from "react";
import { osText } from "@/lib/osText";
import { useAppearanceStore } from "@/state/appearance";
import { IconSquare } from "../icons";
import { Row, RowLabel } from "../primitives";
import { Switch } from "../SettingSwitch";
import { InlineError } from "../states";
import { AppearanceTiles } from "./AppearanceTiles";
import { SettingsSection } from "./SettingsSection";
import { ConfigProblemNote, useConfigProblem } from "./useConfigProblem";

export const GLASS_SETTING_LABEL = "See-through glass";

export function AppearanceSettings() {
  const appearance = useAppearanceStore((state) => state.appearance);
  const loaded = useAppearanceStore((state) => state.loaded);
  const saving = useAppearanceStore((state) => state.saving);
  const error = useAppearanceStore((state) => state.error);
  const save = useAppearanceStore((state) => state.save);
  const themeLabel = useId();
  const glassId = useId();
  const busy = !loaded || saving;
  const problem = useConfigProblem("appearance", appearance);

  return (
    <SettingsSection
      title="Appearance"
      after={error ? <InlineError error={error} /> : <ConfigProblemNote problem={problem} />}
    >
      <Row stacked className="gap-5">
        <span className="flex min-w-0 items-center gap-5">
          <IconSquare icon={SunMoon} />
          <span className="flex min-w-0 flex-col gap-1">
            <span className="text-body font-semibold" id={themeLabel}>
              Colours
            </span>
            <span className="text-caption1 text-fg-secondary contrast-more:text-fg-primary">
              {`Applies to meet-ai's own windows, not the rest of ${osText("thisComputer")}.`}
            </span>
          </span>
        </span>
        <AppearanceTiles
          value={appearance.theme}
          disabled={busy}
          labelledBy={themeLabel}
          onChange={(theme) => void save({ theme })}
        />
      </Row>
      <Row>
        <RowLabel
          icon={Sparkles}
          name={<label htmlFor={glassId}>{GLASS_SETTING_LABEL}</label>}
          detail={`The sidebar and the record prompt let a little of what is behind them show through. Off makes them solid. Turning transparency down in ${osText("settings")} makes them solid too.`}
          mono={false}
        />
        <Switch
          id={glassId}
          on={appearance.glass}
          disabled={busy}
          onChange={(glass) => void save({ glass })}
        />
      </Row>
    </SettingsSection>
  );
}
