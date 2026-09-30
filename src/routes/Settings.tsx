/**
 * Settings: which speech engine this Mac will use, and the models it can
 * download.
 *
 * **The route never waits on the probe.** `Environment::discover` is
 * filesystem-only and sub-millisecond, so the page is built out of it
 * immediately. `registry::resolve` shells out to `meet-stt --probe` at a
 * measured median of ~160 ms — long enough to see, short enough that a
 * full-screen spinner would be worse than the wait. So the engine row renders
 * as "Checking…" and fills itself in.
 *
 * Getting this backwards — one command, awaited before paint — is what makes a
 * settings screen feel slow for a reason the user can never see.
 */

import { useCallback, useEffect, useState } from "react";
import {
  downloadModel,
  engineEnvironment,
  engineSelection,
  modelCatalogue,
  onModelProgress,
} from "@/ipc/client";
import type {
  EnvironmentView,
  ModelProgress,
  ModelView,
  SelectionView,
  UiError,
} from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { useAppStore } from "@/state/app";
import { formatBytes } from "@/ui/format";
import { Checking, ErrorState } from "@/ui/states";
import { useChangeFolder } from "@/ui/useChangeFolder";

export function Settings() {
  const restartOnboarding = useAppStore((state) => state.restartOnboarding);
  const meetings = useAppStore((state) => state.meetings);
  const { busy, error, pick } = useChangeFolder();

  return (
    <div className="page">
      <header className="page__header">
        <h1 className="page__title">Settings</h1>
      </header>

      <EngineSummary />

      <section className="section" aria-labelledby="files-heading">
        <h2 className="section__title" id="files-heading">
          Files
        </h2>
        <div className="card card--flush">
          <div className="row" style={{ flexDirection: "column", alignItems: "stretch" }}>
            <div
              style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}
            >
              <span className="row__label">
                <span className="row__name">Meetings folder</span>
                <span className="row__detail">{meetings?.root ?? "~/Meetings"}</span>
              </span>
              <span
                className="row__value"
                style={{ display: "flex", alignItems: "center", gap: "var(--space-4)" }}
              >
                {meetings?.rootExists ? "Exists" : "Created on first recording"}
                <button
                  type="button"
                  className="btn btn--small"
                  disabled={busy}
                  onClick={() => void pick()}
                >
                  {busy ? "Moving…" : "Change…"}
                </button>
              </span>
            </div>
            {error ? <ErrorState error={error} busy={busy} /> : null}
          </div>
          <div className="row">
            <span className="row__label">
              <span className="row__name">Setup</span>
              <span className="row__detail" style={{ fontFamily: "var(--font-ui)" }}>
                Walk through permission and speech setup again
              </span>
            </span>
            <button
              type="button"
              className="btn btn--small"
              onClick={() => void restartOnboarding()}
            >
              Show setup again
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}

/**
 * The engine card. Shared with onboarding's speech step, so the two cannot
 * describe the same machine differently.
 */
export function EngineSummary() {
  const [environment, setEnvironment] = useState<EnvironmentView | null>(null);
  const [selection, setSelection] = useState<SelectionView | null>(null);
  const [selectionError, setSelectionError] = useState<UiError | null>(null);
  const [probing, setProbing] = useState(true);

  const probe = useCallback(async () => {
    setProbing(true);
    try {
      setSelection(await engineSelection());
      setSelectionError(null);
    } catch (thrown) {
      setSelection(null);
      setSelectionError(toUiError(thrown));
    } finally {
      setProbing(false);
    }
  }, []);

  useEffect(() => {
    // The cheap question first, and separately — this one is allowed to be
    // awaited because it touches nothing but the filesystem.
    void engineEnvironment().then(setEnvironment);
    void probe();
  }, [probe]);

  return (
    <section className="section" aria-labelledby="engine-heading">
      <div className="section__header">
        <h2 className="section__title" id="engine-heading">
          Speech
        </h2>
        <p className="section__hint">Everything here runs on this Mac</p>
      </div>

      <div className="card card--flush">
        <div className="row">
          <span className="row__label">
            <span className="row__name">Engine</span>
            <span className="row__detail" style={{ fontFamily: "var(--font-ui)" }}>
              {selection?.reason ?? "Which engine this Mac can use right now"}
            </span>
          </span>
          {/* The row is present from the first paint and only its right-hand
              side changes, so nothing below it moves when the probe returns. */}
          <span className="row__value">
            {probing ? (
              <Checking label="Checking…" />
            ) : selection ? (
              <span className="badge badge--ok">
                {selection.engine === "apple-speech" ? "Apple, built in" : "Whisper"}
              </span>
            ) : (
              <span className="badge badge--warn">Unavailable</span>
            )}
          </span>
        </div>

        <div className="row">
          <span className="row__label">
            <span className="row__name">Speech helper</span>
            <span className="row__detail">{environment?.sidecar ?? "Not found in this build"}</span>
          </span>
          <span className="row__value">{environment?.locale ?? ""}</span>
        </div>
      </div>

      {/* `stt::Error::EngineUnavailable` maps to "This Mac will use the
          downloadable speech model" + Download. The mapping lives in
          ipc/errors.ts; this just renders whichever it returns. */}
      {selectionError ? <ErrorState error={selectionError} onRemedy={() => void probe()} /> : null}

      <ModelList />
    </section>
  );
}

/** One in-flight state per model, keyed by id. */
type ModelState = {
  progress?: ModelProgress;
  error?: UiError;
  busy?: boolean;
};

function ModelList() {
  const [models, setModels] = useState<ModelView[] | null>(null);
  const [state, setState] = useState<Record<string, ModelState>>({});

  const refresh = useCallback(async () => {
    setModels(await modelCatalogue());
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

  if (models === null) return <Checking label="Reading the model list…" />;
  if (models.length === 0) return null;

  return (
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
  );
}

function ModelRow({
  model,
  state,
  onDownload,
}: {
  model: ModelView;
  state: ModelState;
  onDownload: () => void;
}) {
  const { progress, error, busy } = state;
  const downloading = Boolean(busy) && !model.installed;

  return (
    <div className="row" style={{ flexDirection: "column", alignItems: "stretch" }}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}>
        <span className="row__label">
          <span className="row__name">{model.id}</span>
          {/* The size comes from the pinned catalogue, not from a number typed
              into a document — SPEC §2.4's "~1.6 GB" is stale by a factor of
              three. */}
          <span className="row__detail" style={{ fontFamily: "var(--font-ui)" }}>
            {formatBytes(model.bytes)} · downloaded once, then kept
          </span>
        </span>
        {model.installed ? (
          <span className="badge badge--ok">On this Mac</span>
        ) : (
          <button
            type="button"
            className="btn btn--small"
            disabled={downloading}
            onClick={onDownload}
          >
            {downloading ? "Downloading…" : "Download"}
          </button>
        )}
      </div>

      {downloading ? <DownloadProgress progress={progress} total={model.bytes} /> : null}

      {error ? (
        <ErrorState
          error={error}
          busy={downloading}
          // Both "Try again" and "Download again from scratch" call the same
          // command, and the second one is honest: a checksum failure deletes
          // the bad `.part`, so the next attempt really does start over.
          onRemedy={onDownload}
        />
      ) : null}
    </div>
  );
}

function DownloadProgress({
  progress,
  total,
}: {
  progress: ModelProgress | undefined;
  total: number;
}) {
  // No progress event yet means the request has not gone out. Saying
  // "Starting…" is better than a 0% bar that looks stuck.
  if (!progress) {
    return (
      <div className="progress">
        <div className="progress__track">
          <div className="progress__fill progress__fill--indeterminate" />
        </div>
        <p className="progress__label">
          <span>Starting…</span>
          <span>{formatBytes(total)}</span>
        </p>
      </div>
    );
  }

  if (progress.verifying) {
    return (
      <div className="progress">
        <div className="progress__track">
          <div className="progress__fill progress__fill--indeterminate" />
        </div>
        <p className="progress__label">
          <span>Checking the file is the right one…</span>
          <span>{formatBytes(progress.totalBytes)}</span>
        </p>
      </div>
    );
  }

  const fraction =
    progress.totalBytes > 0
      ? Math.min(1, Math.max(0, progress.downloadedBytes / progress.totalBytes))
      : 0;

  return (
    <div className="progress">
      <div
        className="progress__track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(fraction * 100)}
        aria-label="Download progress"
      >
        <div className="progress__fill" style={{ width: `${fraction * 100}%` }} />
      </div>
      <p className="progress__label">
        <span>
          {formatBytes(progress.downloadedBytes)} of {formatBytes(progress.totalBytes)}
        </span>
        <span>{Math.round(fraction * 100)}%</span>
      </p>
    </div>
  );
}
