/**
 * The downloadable whisper models, one row each, with their download progress.
 *
 * The rows double as the whisper model picker (TUR-75). The data lives in
 * {@link useSpeech}, shared with the engine picker above, because picking
 * Whisper there depends on which models are here.
 */

import { AudioLines } from "lucide-react";
import { useId } from "react";
import { IconSquare } from "@/ui/icons";
import { Card, Prose, Row } from "@/ui/primitives";
import { Checking, ErrorState } from "@/ui/states";
import { ModelRow } from "./ModelRow";
import { engineInUse } from "./tags";
import type { Speech } from "./useSpeech";

export const WHISPER_ONLY_NOTE = "Used only when the engine is Whisper.";

export function ModelList({ speech }: { speech: Speech }) {
  const group = useId();
  const { models, listError, choices, downloads } = speech;

  if (listError && models === null) {
    return <ErrorState error={listError} onRemedy={() => void speech.refreshModels()} />;
  }
  if (models === null) return <Checking label="Reading the model list…" />;
  if (models.length === 0) return null;

  const whisperRuns = choices !== null && engineInUse(choices) === "whisper";

  return (
    <>
      <fieldset className="contents">
        <legend className="sr-only">Whisper model</legend>
        {choices && !whisperRuns ? <Prose>{WHISPER_ONLY_NOTE}</Prose> : null}
        <Card flush>
          <Row>
            <span className="flex min-w-0 items-center gap-5">
              <IconSquare icon={AudioLines} />
              <span className="text-body font-semibold">Whisper models</span>
            </span>
          </Row>
          {models.map((model) => {
            const picked = choices?.model === model.id;
            return (
              <ModelRow
                key={model.id}
                group={group}
                model={model}
                state={downloads[model.id] ?? {}}
                picked={picked}
                inUse={picked && whisperRuns && model.installed}
                onPick={() => speech.pickModel(model.id)}
                onDownload={() => void speech.download(model.id)}
              />
            );
          })}
        </Card>
      </fieldset>
      {/* A list that loaded once and then failed to refresh keeps its rows —
          they are still true of what is on disk — and says so underneath. */}
      {listError ? (
        <ErrorState error={listError} onRemedy={() => void speech.refreshModels()} />
      ) : null}
    </>
  );
}
