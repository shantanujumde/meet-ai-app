/**
 * Onboarding step 2: audio permission — and the screen "Fix this" returns to
 * after setup is done (TUR-127).
 *
 * Both grants are named, and "Not checked" is shown as its own answer rather
 * than folded into allowed or denied; see the wizard's comment in
 * `routes/Onboarding.tsx` for why.
 *
 * TUR-51: the words follow the OS. macOS has two grants; Windows guards only
 * the microphone (loopback needs no permission); Linux has none, so the
 * wizard skips this step there and the copy below is only a fallback.
 */

import { ArrowRight, Mic, RefreshCw, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import type { PermissionStatus } from "@/ipc/types";
import { currentOs, type Os } from "@/lib/osText";
import { recordingBlocked } from "@/lib/recordingPermission";
import { IconSquare } from "@/ui/icons";
import { PrivacyButtons } from "@/ui/PrivacyButtons";
import { Button, ButtonRow, Card, Pill, Prose, Row, RowLabel } from "@/ui/primitives";
import { Checking } from "@/ui/states";

const BADGE = {
  granted: { tone: "ok", label: "Allowed" },
  denied: { tone: "danger", label: "Not allowed" },
  unknown: { tone: "warn", label: "Not checked" },
} as const;

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
  const badge = BADGE[state];
  const os = currentOs();

  return (
    <>
      {os === "macos" ? <MacIntro /> : <OtherIntro os={os} />}

      <Card>
        <Row bare>
          <RowLabel icon={Mic} name="Audio permission" detail={status?.detail ?? ""} mono={false} />
          {loading ? <Checking label="Checking…" /> : <Pill tone={badge.tone}>{badge.label}</Pill>}
        </Row>
      </Card>

      {os === "macos" ? (
        <MacNext
          state={state}
          blocked={recordingBlocked(status)}
          onRecheck={onRecheck}
          onNext={onNext}
        />
      ) : (
        <OtherNext os={os} state={state} onRecheck={onRecheck} onNext={onNext} />
      )}
    </>
  );
}

function MacIntro() {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title flex items-center gap-4">
          <IconSquare icon={ShieldCheck} />
          Let meet-ai hear your Mac
        </h1>
      </header>

      <Prose>
        A meeting has two sides, and macOS asks about each one separately.{" "}
        <strong>Microphone</strong> is you talking. <strong>System Audio Recording</strong> is
        everyone else, coming out of your speakers. meet-ai needs both. With only one, half of every
        conversation goes missing, and nothing on screen would tell you which half.
      </Prose>
    </>
  );
}

type NextProps = {
  state: PermissionStatus["state"];
  onRecheck: () => void;
  onNext: () => void;
};

/**
 * `blocked` is a denial that includes the microphone (TUR-87). With only
 * System Audio Recording off meet-ai still records the microphone, so the
 * denied guidance gets a Continue too (TUR-165): otherwise "Skip setup" is
 * the only way on, and it skips the rest of setup.
 */
function MacNext({ state, blocked, onRecheck, onNext }: NextProps & { blocked: boolean }) {
  return (
    <>
      {state === "denied" ? <DeniedPath blocked={blocked} onRecheck={onRecheck} /> : null}

      {state === "denied" && !blocked ? (
        <ButtonRow>
          <Button tone="primary" icon={ArrowRight} onClick={onNext}>
            Continue
          </Button>
        </ButtonRow>
      ) : null}

      {state !== "denied" ? (
        <>
          <Prose>
            The first time you start a recording, macOS will ask. If you say No by accident, this
            screen shows you how to undo it. It takes two clicks in System Settings, and nothing is
            lost in the meantime.
          </Prose>
          <ButtonRow>
            <Button tone="primary" icon={ArrowRight} onClick={onNext}>
              Continue
            </Button>
            {/* Both panes, as on the denied path: the two grants live in
                  different places, and one button can only land on one. */}
            <PrivacyButtons />
          </ButtonRow>
        </>
      ) : null}
    </>
  );
}

/** Windows and Linux: one switch at most, the microphone. */
function OtherIntro({ os }: { os: Exclude<Os, "macos"> }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title flex items-center gap-4">
          <IconSquare icon={Mic} />
          Let meet-ai use your microphone
        </h1>
      </header>
      {os === "windows" ? (
        <Prose>
          Windows asks for one thing: the <strong>Microphone</strong>, which is you talking.
          Everyone else comes out of your speakers, and Windows lets meet-ai record that without a
          permission.
        </Prose>
      ) : (
        <Prose>
          Linux has no audio permissions to grant. meet-ai records as soon as it can find a
          microphone. If it cannot, it says which device failed.
        </Prose>
      )}
    </>
  );
}

function OtherNext({ os, state, onRecheck, onNext }: NextProps & { os: Exclude<Os, "macos"> }) {
  if (state === "denied" && os === "windows") {
    return (
      <div className="state state--error" role="alert">
        <h2 className="state__title">Windows is blocking the microphone</h2>
        <p className="state__body">
          Recording is off until this is fixed. Nothing you have already recorded is affected.
        </p>
        <ol className="flex flex-col gap-5 [counter-reset:step]">
          <Instruction>
            Open <strong>Settings</strong> → <strong>Privacy &amp; security</strong> →{" "}
            <strong>Microphone</strong>. The button below jumps straight there.
          </Instruction>
          <Instruction>
            Turn on <strong>Microphone access</strong> and{" "}
            <strong>Let desktop apps access your microphone</strong>.
          </Instruction>
          <Instruction>
            Come back here and choose <strong>Check again</strong>.
          </Instruction>
        </ol>
        <ButtonRow>
          <PrivacyButtons primary="microphone" />
          <Button icon={RefreshCw} onClick={onRecheck}>
            Check again
          </Button>
        </ButtonRow>
      </div>
    );
  }
  return (
    <ButtonRow>
      <Button tone="primary" icon={ArrowRight} onClick={onNext}>
        Continue
      </Button>
      <PrivacyButtons />
    </ButtonRow>
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
function DeniedPath({ blocked, onRecheck }: { blocked: boolean; onRecheck: () => void }) {
  return (
    <div className="state state--error" role="alert">
      {blocked ? (
        <>
          <h2 className="state__title">meet-ai is not allowed to record audio</h2>
          <p className="state__body">
            Recording is off until this is fixed, because meet-ai will not start a recording it
            knows would be silent. Nothing you have already recorded is affected.
          </p>
        </>
      ) : (
        <>
          <h2 className="state__title">meet-ai is not allowed to record system audio</h2>
          <p className="state__body">
            Recordings capture only your microphone until this is fixed, so the other people in your
            call are missing. You can continue setup now and fix it later.
          </p>
        </>
      )}

      <ol className="flex flex-col gap-5 [counter-reset:step]">
        <Instruction>
          Open <strong>System Settings</strong> → <strong>Privacy &amp; Security</strong>. The
          buttons below jump straight there.
        </Instruction>
        <Instruction>
          Find <strong>System Audio Recording Only</strong> and switch <strong>meet-ai</strong> on.
          This is the one that captures the other people in your call.
        </Instruction>
        <Instruction>
          Go back and find <strong>Microphone</strong>, and switch <strong>meet-ai</strong> on there
          too. This one captures you.
        </Instruction>
        <Instruction>
          Come back here and choose <strong>Check again</strong>. If macOS asks you to quit and
          reopen meet-ai first, do that. The change does not always work while the app is running.
        </Instruction>
      </ol>

      <ButtonRow>
        <PrivacyButtons primary="audio-capture" />
        <Button icon={RefreshCw} onClick={onRecheck}>
          Check again
        </Button>
      </ButtonRow>
    </div>
  );
}

/** One numbered step. The number is a CSS counter, drawn in a chip beside the text. */
function Instruction({ children }: { children: ReactNode }) {
  return (
    <li
      className={[
        "grid grid-cols-[var(--control-h-regular)_1fr] items-start gap-5",
        "text-body leading-prose text-fg-secondary [counter-increment:step]",
        "before:grid before:size-(--control-h-regular) before:place-items-center before:rounded-capsule",
        "before:bg-glass-sunken before:text-caption1 before:font-semibold before:tabular-nums",
        "before:text-fg-primary before:content-[counter(step)]",
        "[&_strong]:font-semibold [&_strong]:text-fg-primary",
      ].join(" ")}
    >
      <span>{children}</span>
    </li>
  );
}
