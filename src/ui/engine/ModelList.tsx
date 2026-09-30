/**
 * The downloadable speech models, one row each, with their download progress.
 */

import { useCallback, useEffect, useState } from "react";
import { downloadModel, modelCatalogue, onModelProgress } from "@/ipc/client";
import type { ModelView, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Checking, ErrorState } from "@/ui/states";
import { ModelRow, type ModelState } from "./ModelRow";

export function ModelList() {
  const [models, setModels] = useState<ModelView[] | null>(null);
  const [listError, setListError] = useState<UiError | null>(null);
  const [state, setState] = useState<Record<string, ModelState>>({});

  // Catches its own failure: an unreadable catalogue used to be an unhandled
  // rejection that left "Reading the model list…" on screen for good.
  const refresh = useCallback(async () => {
    try {
      setModels(await modelCatalogue());
      setListError(null);
    } catch (thrown) {
      setListError(toUiError(thrown));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // `modelfetch::ensure` fires `on_progress` once before the first request goes
  // out, and again with `verifying: true` before hashing. Both arrive here, so
  // a bar never sits at 0% with no explanation or at 100% looking frozen.
  useEffect(() => {
    return onModelProgress((progress) => {
      setState((previous) => ({
        ...previous,
        [progress.modelId]: { ...previous[progress.modelId], progress },
      }));
    });
  }, []);

  const start = useCallback(
    async (id: string) => {
      setState((previous) => ({ ...previous, [id]: { busy: true } }));
      try {
        await downloadModel(id);
        setState((previous) => ({ ...previous, [id]: {} }));
        await refresh();
      } catch (thrown) {
        setState((previous) => ({
          ...previous,
          [id]: { error: toUiError(thrown), busy: false },
        }));
      }
    },
    [refresh],
  );

  if (listError && models === null) {
    return <ErrorState error={listError} onRemedy={() => void refresh()} />;
  }
  if (models === null) return <Checking label="Reading the model list…" />;
  if (models.length === 0) return null;

  return (
    <>
      <div className="card card--flush">
        {models.map((model) => (
          <ModelRow
            key={model.id}
            model={model}
            state={state[model.id] ?? {}}
            onDownload={() => void start(model.id)}
          />
        ))}
      </div>
      {/* A list that loaded once and then failed to refresh keeps its rows —
          they are still true of what is on disk — and says so underneath. */}
      {listError ? <ErrorState error={listError} onRemedy={() => void refresh()} /> : null}
    </>
  );
}
