/**
 * Settings → Calendars: where meetings come from (TUR-49, SPEC A12).
 *
 * - macOS: **Calendar app** (EventKit, every account already in Calendar.app,
 *   on by default), plus optional Google and Microsoft sign-ins.
 * - Windows and Linux: only the two sign-ins; there is no Calendar app row.
 *
 * Each sign-in row shows its account and one action: sign in, disconnect,
 * or "Sign in again" when the provider rejected the stored sign-in. A
 * provider with no client id in `config.jsonc` names the key to add instead
 * of offering a button that cannot work. Connecting adds the provider to
 * `calendar.providers` and disconnecting removes it, so Today, reminders,
 * auto-titles and the brief follow without a restart.
 *
 * The sources are a config read and paint at once; the accounts can wait
 * on the network after a launch, so each row says it is checking until then.
 */

import { Calendar } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";
import { useIpcValue } from "@/hooks/useIpcValue";
import {
  type CalendarAccount,
  type CalendarSources,
  calendarAccounts,
  calendarDisconnect,
  calendarSources,
  type SettingsSnapshot,
  SIGN_IN_PROVIDERS,
  type SignInProvider,
  setCalendarApp,
} from "@/ipc/client";
import { toUiError } from "@/ipc/types";
import { Button, ButtonRow, Row, RowLabel } from "../primitives";
import { SettingSwitch } from "../SettingSwitch";
import { SettingsSection } from "../settings/SettingsSection";
import { useSnapshotLoad } from "../settings/snapshot";
import { Checking, InlineError } from "../states";
import { notConfiguredCopy, PROVIDER_NAME, signInLabel } from "./copy";
import { CANCEL_SIGN_IN, WAITING_FOR_BROWSER } from "./SignInButtons";
import { useCalendarSignIn } from "./useCalendarSignIn";

export const CALENDAR_APP_LABEL = "Calendar app";
export const SIGN_IN_EXPIRED = "Sign-in expired";
export const NOT_REMEMBERED =
  "Signed in until meet-ai quits: this system has no keystore to remember it";

const pickSources = (snapshot: SettingsSnapshot) => snapshot.calendarSources;

export function CalendarSettings() {
  const loadSources = useSnapshotLoad(pickSources, calendarSources);
  const {
    value: sources,
    setValue: setSources,
    error: sourcesError,
  } = useIpcValue<CalendarSources>(loadSources);
  const read = useIpcValue<CalendarAccount[]>(calendarAccounts);
  const setAccounts = read.setValue;
  // Accounts that could not be read: every row offers a sign-in.
  const accounts = read.value ?? (read.error ? [] : null);
  const error = sourcesError ?? read.error;

  const replaceAccount = (account: CalendarAccount) =>
    setAccounts((current) => [
      ...(current ?? []).filter((each) => each.provider !== account.provider),
      account,
    ]);

  return (
    <SettingsSection
      title="Calendars"
      anchorId="calendars"
      description="Where meetings come from"
      after={error ? <InlineError error={error} /> : null}
    >
      {sources === null ? (
        error ? null : (
          <Row>
            <Checking label="Reading your calendar settings…" />
          </Row>
        )
      ) : (
        <>
          {sources.calendarAppAvailable ? (
            <CalendarAppRow on={sources.calendarApp} onSaved={setSources} />
          ) : null}
          {SIGN_IN_PROVIDERS.map((provider) => (
            <SignInRow
              key={provider}
              provider={provider}
              configured={sources.configured.includes(provider)}
              account={
                accounts === null ? undefined : accounts.find((a) => a.provider === provider)
              }
              onAccount={(account) => {
                replaceAccount(account);
                // The provider is now in `calendar.providers`.
                void calendarSources().then(setSources, () => {});
              }}
              onSources={setSources}
            />
          ))}
        </>
      )}
    </SettingsSection>
  );
}

/**
 * The Calendar app (macOS): a switch, like the Dock setting. `on` is whether
 * `"eventkit"` is in `calendar.providers`, from the sources the card already
 * read (TUR-171), not a read of its own.
 */
function CalendarAppRow({
  on,
  onSaved,
}: {
  on: boolean;
  onSaved: (sources: CalendarSources) => void;
}) {
  const load = useCallback(async () => on, [on]);
  return (
    <SettingSwitch
      icon={Calendar}
      label={CALENDAR_APP_LABEL}
      detail="Every account already in Calendar.app on this Mac. No sign-in needed."
      load={load}
      save={async (on) => {
        const saved = await setCalendarApp(on);
        onSaved(saved);
        return saved.calendarApp;
      }}
    />
  );
}

/** One sign-in: its account, and the one thing to do next. */
function SignInRow({
  provider,
  configured,
  account,
  onAccount,
  onSources,
}: {
  provider: SignInProvider;
  configured: boolean;
  /** `undefined` while the accounts are still being read. */
  account: CalendarAccount | undefined;
  onAccount: (account: CalendarAccount) => void;
  onSources: (sources: CalendarSources) => void;
}) {
  const { connecting, error, setError, connect, cancel } = useCalendarSignIn(onAccount);
  const [disconnecting, setDisconnecting] = useState(false);
  const busy = connecting !== null || disconnecting;
  const name = PROVIDER_NAME[provider];

  async function disconnect() {
    setDisconnecting(true);
    setError(null);
    try {
      onSources(await calendarDisconnect(provider));
      onAccount({ provider, account: null, state: "signed-out", remembered: true });
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setDisconnecting(false);
    }
  }

  const state = account?.state ?? "signed-out";
  const connectButton = (label: string, tone: "primary" | "neutral") => {
    const button = (
      <Button size="small" tone={tone} disabled={busy} onClick={() => void connect(provider)}>
        {connecting ? WAITING_FOR_BROWSER : label}
      </Button>
    );
    // TUR-174: a closed browser tab never comes back; Cancel stops waiting.
    if (!connecting) return button;
    return (
      <ButtonRow>
        {button}
        <Button size="small" tone="quiet" onClick={() => void cancel()}>
          {CANCEL_SIGN_IN}
        </Button>
      </ButtonRow>
    );
  };
  const disconnectButton = (
    <Button size="small" tone="quiet" disabled={busy} onClick={() => void disconnect()}>
      {disconnecting ? "Disconnecting…" : "Disconnect"}
    </Button>
  );

  let detail: string;
  let action: ReactNode;
  if (!configured && state === "signed-out") {
    detail = notConfiguredCopy(provider);
    action = null;
  } else if (account === undefined) {
    detail = "";
    action = <Checking label="Checking sign-in…" />;
  } else if (state === "expired") {
    detail = account.account ? `${account.account} · ${SIGN_IN_EXPIRED}` : SIGN_IN_EXPIRED;
    action = (
      <ButtonRow>
        {connectButton("Sign in again", "primary")}
        {disconnectButton}
      </ButtonRow>
    );
  } else if (state === "signed-in") {
    const who = account.account ?? "Signed in";
    detail = account.remembered ? who : `${who} · ${NOT_REMEMBERED}`;
    action = disconnectButton;
  } else {
    detail = "Not connected";
    action = connectButton(signInLabel(provider), "neutral");
  }

  return (
    <>
      <Row role="group" aria-label={name}>
        <RowLabel icon={Calendar} name={name} detail={detail} mono={false} />
        {action}
      </Row>
      {error ? (
        <div className="px-6 pb-5">
          <InlineError error={error} />
        </div>
      ) : null}
    </>
  );
}
