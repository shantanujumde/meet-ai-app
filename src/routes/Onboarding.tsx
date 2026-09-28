/**
 * Onboarding, and the screen a user lands on after saying No to audio capture.
 *
 * SPEC §8.1 makes this its own route in v1 rather than a first-launch special
 * case, and makes the denied path a requirement rather than an edge case: one
 * sentence on what breaks, a **Retry** button, and **Open System Settings**
 * deep-linking to Privacy & Security.
 *
 * Two things shape the copy here:
 *
 * * **macOS needs two separate grants.** The microphone carries the `You`
 *   channel and the system audio tap carries `Others` (L5). Granting one and
 *   not the other produces a half-recorded meeting, so both are named.
 * * **"Not checked" is not "denied".** On denial every status code is `noErr`
 *   and the tap returns silence, so only a positive control tone proves
 *   permission (SPEC §8.1) — the backend plays it and listens on every check.
 *   If that check cannot run at all (no output device, a read failure), the
 *   screen still says it does not know, rather than claiming a grant it has
 *   not verified.
 */

import { useEffect } from "react";
import { useNavigate, useParams } from "react-router";
import { openPrivacySettings, revealMeeting } from "@/ipc/client";
import type { PermissionStatus } from "@/ipc/types";
import { useAppStore } from "@/state/app";
import { Checking, ErrorState } from "@/ui/states";
import { useChangeFolder } from "@/ui/useChangeFolder";
import { EngineSummary } from "./Settings";

const STEPS = ["welcome", "permission", "speech", "folder"] as const;
type Step = (typeof STEPS)[number];

function isStep(value: string | undefined): value is Step {
  return STEPS.includes(value as Step);
}

export function Onboarding() {
  const { step } = useParams<{ step?: string }>();
  const navigate = useNavigate();
  const current: Step = isStep(step) ? step : "welcome";
  const index = STEPS.indexOf(current);

  const permission = useAppStore((state) => state.permission);
  const permissionLoading = useAppStore((state) => state.permissionLoading);
  const loadPermission = useAppStore((state) => state.loadPermission);
  const finish = useAppStore((state) => state.finishOnboarding);

  // Re-check every time this route is opened. Someone who just came back from
  // System Settings must not be shown a cached "denied".
  useEffect(() => {
    void loadPermission();
  }, [loadPermission]);

  function goNext() {
    const next = STEPS[index + 1];
    if (next) navigate(`/onboarding/${next}`);
  }

  async function complete() {
    await finish();
    navigate("/meetings");
  }

  return (
    <div className="page page--narrow">
      {/* The dots are decoration; the sentence is the accessible version.
          An aria-label on a plain div is not announced reliably anyway. */}
      <p className="sr-only">
        Step {index + 1} of {STEPS.length}
      </p>
      <div className="progress-dots" aria-hidden="true">
        {STEPS.map((name, position) => (
          <span key={name} data-active={position <= index} />
        ))}
      </div>

      {current === "welcome" ? <Welcome onNext={goNext} /> : null}
      {current === "permission" ? (
        <Permission
          status={permission}
          loading={permissionLoading}
          onRecheck={() => void loadPermission()}
          onNext={goNext}
        />
      ) : null}
      {current === "speech" ? <Speech onNext={goNext} /> : null}
      {current === "folder" ? <Folder onFinish={() => void complete()} /> : null}

      <div className="btn-row">
        {index > 0 ? (
          <button
            type="button"
            className="btn btn--quiet btn--small"
            onClick={() => navigate(`/onboarding/${STEPS[index - 1]}`)}
          >
            Back
          </button>
        ) : null}
        <button type="button" className="btn btn--quiet btn--small" onClick={() => void complete()}>
          Skip setup
        </button>
      </div>
    </div>
  );
}

function Welcome({ onNext }: { onNext: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title">meet-ai records your meetings</h1>
      </header>
      <p className="prose">
        It listens to your microphone and to whatever your Mac is playing, writes both sides out as
        plain markdown, and stops. There is no bot in your call and no account to make.
      </p>
      <p className="prose">
        Nothing is uploaded. Recordings, transcripts and notes stay in a folder on this Mac, in
        files you can open in any editor. The only thing meet-ai ever downloads is a speech model,
        and only if this Mac needs one.
      </p>
      <p className="prose">Two things to set up, then you are done.</p>
      <div className="btn-row">
        <button type="button" className="btn btn--primary" onClick={onNext}>
          Get started
        </button>
      </div>
    </>
  );
}

function Permission({
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
            <button
              type="button"
              className="btn"
              onClick={() => void openSettings("audio-capture")}
            >
              Open System Settings
            </button>
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
        <button
          type="button"
          className="btn btn--primary"
          onClick={() => void openSettings("audio-capture")}
        >
          Open System Audio Recording
        </button>
        <button type="button" className="btn" onClick={() => void openSettings("microphone")}>
          Open Microphone
        </button>
        <button type="button" className="btn" onClick={onRecheck}>
          Check again
        </button>
      </div>
    </div>
  );
}

async function openSettings(pane: "audio-capture" | "microphone") {
  try {
    await openPrivacySettings(pane);
  } catch {
    // The deep link failed and so did the fallback to the pane root. The
    // written steps above still work — they name the panes rather than relying
    // on the button — so this is not worth an error screen on top of an error
    // screen.
  }
}

function Speech({ onNext }: { onNext: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title">How meet-ai turns speech into text</h1>
      </header>
      <p className="prose">
        This happens on your Mac, not on a server. Newer Macs have Apple's speech engine built in
        and need nothing at all; older ones use a model meet-ai downloads once and then keeps.
      </p>

      <EngineSummary />

      <div className="btn-row">
        <button type="button" className="btn btn--primary" onClick={onNext}>
          Continue
        </button>
      </div>
    </>
  );
}

function Folder({ onFinish }: { onFinish: () => void }) {
  const meetings = useAppStore((state) => state.meetings);
  const root = meetings?.root ?? "~/Meetings";
  const { busy, error, pick } = useChangeFolder();

  return (
    <>
      <header className="page__header">
        <h1 className="page__title">Where your meetings live</h1>
      </header>
      <p className="prose">
        Every meeting becomes a folder here, holding the transcript, your notes and the audio. They
        are ordinary markdown files: open them in any editor, search them with Spotlight, keep them
        in a git repo if you want.
      </p>
      <div className="card">
        <div className="row" style={{ padding: 0, flexDirection: "column", alignItems: "stretch" }}>
          <div style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}>
            <span className="row__label">
              <span className="row__name">Meetings folder</span>
              <span className="row__detail">{root}</span>
            </span>
            <span
              className="row__value"
              style={{ display: "flex", alignItems: "center", gap: "var(--space-4)" }}
            >
              {meetings?.rootExists ? (
                <button
                  type="button"
                  className="btn btn--small"
                  onClick={() => void revealFirstMeeting()}
                >
                  Show in Finder
                </button>
              ) : (
                <span className="badge">Created on first recording</span>
              )}
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
      </div>
      <p className="prose">
        Audio is deleted after 7 days by default; the text is kept forever. Press{" "}
        <strong>⌘⇧R</strong> from anywhere to start and stop — you do not need this window open, or
        even visible.
      </p>
      <div className="btn-row">
        <button type="button" className="btn btn--primary" onClick={onFinish}>
          Done
        </button>
      </div>
    </>
  );
}

/** Open the newest meeting's folder, which is the quickest route into the root. */
async function revealFirstMeeting() {
  const first = useAppStore.getState().meetings?.meetings[0];
  if (!first) return;
  try {
    await revealMeeting(first.id);
  } catch {
    // Finder refused or the folder moved. The path is printed above either way.
  }
}
