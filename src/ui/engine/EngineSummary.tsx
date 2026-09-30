/**
 * The engine card: which speech engine this Mac will use, and the models it
 * can download.
 *
 * Shared by Settings and onboarding's speech step, so the two cannot describe
 * the same machine differently. It lives here rather than in either route so
 * neither route imports the other.
 *
 * **It never waits on the probe.** `Environment::discover` is filesystem-only
 * and sub-millisecond, so the card is built out of it immediately.
 * `registry::resolve` shells out to `meet-stt --probe` at a measured median of
 * ~160 ms — long enough to see, short enough that a full-screen spinner would
 * be worse than the wait. So the engine row renders as "Checking…" and fills
 * itself in.
 */

import { useCallback, useEffect, useState } from "react";
import { engineEnvironment, engineSelection } from "@/ipc/client";
import type { EnvironmentView, SelectionView, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Checking, ErrorState } from "@/ui/states";
import { ModelList } from "./ModelList";

export function EngineSummary() {
  const [environment, setEnvironment] = useState<EnvironmentView | null>(null);
  const [environmentError, setEnvironmentError] = useState<UiError | null>(null);
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
    // awaited because it touches nothing but the filesystem. A failure here
    // used to be an unhandled rejection that left "Not found in this build"
    // on screen, which is a claim about the build nobody had checked.
    engineEnvironment()
      .then((view) => {
        setEnvironment(view);
        setEnvironmentError(null);
      })
      .catch((thrown: unknown) => setEnvironmentError(toUiError(thrown)));
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

      {environmentError ? <ErrorState error={environmentError} /> : null}

      {/* `stt::Error::EngineUnavailable` maps to "This Mac will use the
          downloadable speech model" + Download. The mapping lives in
          ipc/errors.ts; this just renders whichever it returns. */}
      {selectionError ? <ErrorState error={selectionError} onRemedy={() => void probe()} /> : null}

      <ModelList />
    </section>
  );
}
