/**
 * The state behind the speech card: the engine picker, the model list and
 * their downloads, and saving a pick (TUR-75).
 *
 * The picker and the list share one hook because they depend on each other:
 * Whisper can only be picked once a model is downloaded, and picking Whisper
 * also has to pick a downloaded model.
 *
 * A pick shows at once and is saved behind it; if the save is refused, the
 * radios go back to what is on disk and the reason is shown.
 */

import { useCallback, useEffect, useState } from "react";
import {
  downloadModel,
  engineChoices,
  engineEnvironment,
  engineSelection,
  modelCatalogue,
  onModelProgress,
  setTranscription,
} from "@/ipc/client";
import type { EngineChoice, EngineChoices, EnvironmentView, ModelView, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import type { ModelState } from "./ModelRow";

export type Speech = ReturnType<typeof useSpeech>;

export function useSpeech() {
  const [environment, setEnvironment] = useState<EnvironmentView | null>(null);
  const [environmentError, setEnvironmentError] = useState<UiError | null>(null);
  const [choices, setChoices] = useState<EngineChoices | null>(null);
  const [choicesError, setChoicesError] = useState<UiError | null>(null);
  const [selectionError, setSelectionError] = useState<UiError | null>(null);
  const [checking, setChecking] = useState(true);
  const [saveError, setSaveError] = useState<UiError | null>(null);
  const [models, setModels] = useState<ModelView[] | null>(null);
  const [listError, setListError] = useState<UiError | null>(null);
  const [downloads, setDownloads] = useState<Record<string, ModelState>>({});

  /**
   * The probe-backed half: the picker, and whether the saved engine can run.
   * The second is the old card's error row ("This Mac will use the
   * downloadable speech model"), kept because a saved Whisper whose model was
   * since removed is only caught there.
   */
  const check = useCallback(async () => {
    setChecking(true);
    const [picked, selected] = await Promise.allSettled([engineChoices(), engineSelection()]);
    if (picked.status === "fulfilled") {
      setChoices(picked.value);
      setChoicesError(null);
    } else {
      setChoicesError(toUiError(picked.reason));
    }
    setSelectionError(selected.status === "rejected" ? toUiError(selected.reason) : null);
    setChecking(false);
  }, []);

  // Catches its own failure: an unreadable catalogue used to be an unhandled
  // rejection that left "Reading the model list…" on screen for good.
  const refreshModels = useCallback(async () => {
    try {
      setModels(await modelCatalogue());
      setListError(null);
    } catch (thrown) {
      setListError(toUiError(thrown));
    }
  }, []);

  useEffect(() => {
    // The cheap question first, and separately: filesystem only, so it is
    // allowed to be awaited before the probe answers.
    engineEnvironment()
      .then((view) => {
        setEnvironment(view);
        setEnvironmentError(null);
      })
      .catch((thrown: unknown) => setEnvironmentError(toUiError(thrown)));
    void refreshModels();
    void check();
  }, [check, refreshModels]);

  // `modelfetch::ensure` fires `on_progress` once before the first request goes
  // out, and again with `verifying: true` before hashing. Both arrive here, so
  // a bar never sits at 0% with no explanation or at 100% looking frozen.
  useEffect(() => {
    return onModelProgress((progress) => {
      setDownloads((previous) => ({
        ...previous,
        [progress.modelId]: { ...previous[progress.modelId], progress },
      }));
    });
  }, []);

  const save = useCallback(
    async (engine: EngineChoice, model: string) => {
      if (!choices) return;
      const before = choices;
      setChoices({ ...choices, engine, model });
      setSaveError(null);
      try {
        setChoices(await setTranscription(engine, model));
        // Whether the saved engine can run is the probe's answer, not ours.
        engineSelection()
          .then(() => setSelectionError(null))
          .catch((thrown: unknown) => setSelectionError(toUiError(thrown)));
      } catch (thrown) {
        setChoices(before);
        setSaveError(toUiError(thrown));
      }
    },
    [choices],
  );

  /** Whisper also needs a model: the saved one if it is here, else the best one that is. */
  const pickEngine = useCallback(
    (engine: EngineChoice) => {
      if (!choices) return;
      let model = choices.model;
      if (engine === "whisper") {
        const installed = (models ?? []).filter((m) => m.installed);
        if (!installed.some((m) => m.id === model)) {
          const pick = installed.find((m) => m.recommended !== null) ?? installed[0];
          if (!pick) return;
          model = pick.id;
        }
      }
      void save(engine, model);
    },
    [choices, models, save],
  );

  const pickModel = useCallback(
    (id: string) => {
      if (!choices) return;
      void save(choices.engine, id);
    },
    [choices, save],
  );

  const download = useCallback(
    async (id: string) => {
      setDownloads((previous) => ({ ...previous, [id]: { busy: true } }));
      try {
        await downloadModel(id);
        setDownloads((previous) => ({ ...previous, [id]: {} }));
        await refreshModels();
        // A first model makes Whisper pickable.
        void check();
      } catch (thrown) {
        setDownloads((previous) => ({
          ...previous,
          [id]: { error: toUiError(thrown), busy: false },
        }));
      }
    },
    [check, refreshModels],
  );

  return {
    environment,
    environmentError,
    choices,
    choicesError,
    selectionError,
    checking,
    saveError,
    models,
    listError,
    downloads,
    check,
    refreshModels,
    pickEngine,
    pickModel,
    download,
  };
}
