/**
 * Onboarding step 2: audio permission — and the screen "Fix this" returns to
 * after setup is done (TUR-127).
 *
 * Both grants are named, and "Not checked" is shown as its own answer rather
 * than folded into allowed or denied; see the wizard's comment in
 * `routes/Onboarding.tsx` for why.
 */

import type { PermissionStatus } from "@/ipc/types";
import { PrivacyButtons } from "@/ui/PrivacyButtons";
import { Checking } from "@/ui/states";

export function PermissionStep({
  status,
  loading,
  onRecheck,
  onNext,
}: {
  status: PermissionStatus | null;
  loading: boolean;
  onRecheck: () => void;
  onNext: () => void;
}) {
  const state = status?.state ?? "unknown";

  return (
    <>
      <header className="page__header">
        <h1 className="page__title">Let meet-ai hear your Mac</h1>
      </header>

      <p className="prose">
        A meeting has two sides, and macOS guards them separately. <strong>Microphone</strong> is
        you talking. <strong>System Audio Recording</strong> is everyone else, coming out of your
        speakers. meet-ai needs both — with only one, half of every conversation goes missing and
        nothing on screen would tell you which half.
      </p>

      <div className="card">
        <div className="row" style={{ padding: 0 }}>
          <span className="row__label">
            <span className="row__name">Audio permission</span>
            <span className="row__detail" style={{ fontFamily: "var(--font-ui)" }}>
              {status?.detail ?? ""}
            </span>
          </span>
          {loading ? (
            <Checking label="Checking…" />
          ) : (
            <span
              className={`badge badge--${state === "granted" ? "ok" : state === "denied" ? "danger" : "warn"}`}
            >
              {state === "granted" ? "Allowed" : state === "denied" ? "Not allowed" : "Not checked"}
            </span>
          )}
        </div>
      </div>

      {state === "denied" ? <DeniedPath onRecheck={onRecheck} /> : null}

      {state !== "denied" ? (
        <>
          <p className="prose">
            The first time you start a recording, macOS will ask. If you say No by accident, this
            screen shows you how to undo it — it is two clicks in System Settings, and nothing is
            lost in the meantime.
          </p>
          <div className="btn-row">
            <button type="button" className="btn btn--primary" onClick={onNext}>
              Continue
            </button>
            {/* Both panes, as on the denied path: the two grants live in
                different places, and one button can only land on one. */}
            <PrivacyButtons />
          </div>
        </>
      ) : null}
    </>
  );
}

/**
 * The screen the charter calls out: the user said No, and now has to undo it.
 *
 * Numbered steps naming the exact panes and switches, because "grant the
 * permission in System Settings" is not instructions — Privacy & Security has
 * two dozen entries and the audio-capture one is not called what the prompt
 * called it.
 */
function DeniedPath({ onRecheck }: { onRecheck: () => void }) {
  return (
    <div className="state state--error" role="alert">
      <h2 className="state__title">meet-ai is not allowed to record audio</h2>
      <p className="state__body">
        Recording is switched off until this is fixed — meet-ai will not start a recording it knows
        would capture silence. Nothing you have already recorded is affected.
      </p>

      <ol className="steps">
        <li>
          <span>
            Open <strong>System Settings</strong> → <strong>Privacy &amp; Security</strong>. The
            buttons below jump straight there.
          </span>
        </li>
        <li>
          <span>
            Find <strong>System Audio Recording Only</strong> and switch <strong>meet-ai</strong>{" "}
            on. This is the one that captures the other people in your call.
          </span>
        </li>
        <li>
          <span>
            Go back and find <strong>Microphone</strong>, and switch <strong>meet-ai</strong> on
            there too. This one captures you.
          </span>
        </li>
        <li>
          <span>
            Come back here and choose <strong>Check again</strong>. If macOS asks you to quit and
            reopen meet-ai first, do that — the change does not always take effect while the app is
            running.
          </span>
        </li>
      </ol>

      <div className="btn-row">
        <PrivacyButtons primary="audio-capture" />
        <button type="button" className="btn" onClick={onRecheck}>
          Check again
        </button>
      </div>
    </div>
  );
}
