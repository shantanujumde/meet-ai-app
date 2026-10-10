/**
 * The speech card: which engine recordings use (a picker, TUR-75), and the
 * whisper models with what each is good for (TUR-79).
 *
 * Shared by Settings and onboarding's speech step, so the two cannot describe
 * the same machine differently. It lives here rather than in either route so
 * neither route imports the other.
 *
 * **It never waits on the probe.** `Environment::discover` is filesystem-only
 * and sub-millisecond, so the card is built out of it immediately.
 * `registry::options` shells out to `meet-stt --probe` at a measured median of
 * ~160 ms — long enough to see, short enough that a full-screen spinner would
 * be worse than the wait. So the picker is drawn at once with its radios held
 * and "Checking…" beside Automatic, and fills itself in.
 */

import { Info } from "lucide-react";
import { osText } from "@/lib/osText";
import { Prose, Row, RowLabel, RowValue } from "@/ui/primitives";
import { SettingsSection } from "@/ui/settings/SettingsSection";
import { ErrorState } from "@/ui/states";
import { EnginePicker } from "./EnginePicker";
import { ModelList } from "./ModelList";
import { SpokenLanguagePicker } from "./SpokenLanguagePicker";
import { useSpeech } from "./useSpeech";

export const NEXT_RECORDING_NOTE =
  "Changes apply from the next recording. A recording already running keeps its engine.";

export function EngineSummary() {
  const speech = useSpeech();
  const { environment, choices } = speech;

  return (
    <SettingsSection
      title="Speech"
      description={`Everything here runs on ${osText("thisComputer")}`}
      after={
        <>
          <Prose>{NEXT_RECORDING_NOTE}</Prose>

          {speech.saveError ? <ErrorState error={speech.saveError} /> : null}
          {speech.choicesError ? (
            <ErrorState error={speech.choicesError} onRemedy={() => void speech.check()} />
          ) : null}
          {/* `stt::Error::EngineUnavailable` maps to "This Mac will use the
              downloadable speech model" + Download. The mapping lives in
              ipc/errors.ts; this just renders whichever it returns. */}
          {speech.selectionError ? (
            <ErrorState error={speech.selectionError} onRemedy={() => void speech.check()} />
          ) : null}

          <ModelList speech={speech} />

          {/* Debugging facts, not settings: folded away unless asked for. */}
          <details>
            <summary className="w-fit cursor-default text-footnote text-fg-secondary hover:text-fg-primary">
              Details
            </summary>
            <Row bare className="pt-4">
              <RowLabel
                icon={Info}
                name="Speech helper"
                detail={environment?.sidecar ?? "Not found in this build"}
              />
              <RowValue>{environment?.locale ?? ""}</RowValue>
            </Row>
          </details>
          {speech.environmentError ? <ErrorState error={speech.environmentError} /> : null}
        </>
      }
    >
      <EnginePicker
        choices={choices}
        checking={speech.checking}
        onPick={speech.pickEngine}
        parakeetDownload={choices ? (speech.downloads[choices.parakeetModel.id] ?? {}) : {}}
        onDownloadParakeet={() => {
          if (choices) void speech.download(choices.parakeetModel.id);
        }}
        onCancelParakeet={() => {
          if (choices) void speech.cancel(choices.parakeetModel.id);
        }}
      />
      <SpokenLanguagePicker
        choices={choices}
        onPick={(language) => void speech.pickLanguage(language)}
      />
    </SettingsSection>
  );
}
