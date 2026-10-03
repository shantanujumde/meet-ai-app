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

import { type ReactNode, useCallback, useEffect, useId, useState } from "react";
import {
  type CalendarAccount,
  type CalendarSources,
  calendarAccounts,
  calendarConnect,
  calendarDisconnect,
  calendarSources,
  SIGN_IN_PROVIDERS,
  type SignInProvider,
  setCalendarApp,
} from "@/ipc/client";
import { toUiError, type UiError } from "@/ipc/types";
import { cn } from "@/lib/cn";
import { Button, ButtonRow, Card, Row, RowLabel } from "../primitives";
import { Checking, InlineError } from "../states";
import { notConfiguredCopy, PROVIDER_NAME, signInError, signInLabel } from "./copy";
import { WAITING_FOR_BROWSER } from "./SignInButtons";

export const CALENDAR_APP_LABEL = "Calendar app";
export const SIGN_IN_EXPIRED = "Sign-in expired";
export const NOT_REMEMBERED =
  "Signed in until meet-ai quits: this system has no keystore to remember it";

export function CalendarSettings() {
  const [sources, setSources] = useState<CalendarSources | null>(null);
  const [accounts, setAccounts] = useState<CalendarAccount[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  const load = useCallback(() => {
    let live = true;
    calendarSources().then(
      (answer) => {
        if (live) setSources(answer);
      },
      (thrown: unknown) => {
        if (live) setError(toUiError(thrown));
      },
    );
    calendarAccounts().then(
      (answer) => {
        if (live) setAccounts(answer);
      },
      (thrown: unknown) => {
        if (!live) return;
        setAccounts([]);
        setError(toUiError(thrown));
      },
    );
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => load(), [load]);

  const replaceAccount = (account: CalendarAccount) =>
    setAccounts((current) => [
      ...(current ?? []).filter((each) => each.provider !== account.provider),
      account,
    ]);

  return (
    <section className="section" aria-labelledby="calendars-heading">
      <div className="section__header">
        <h2 className="section__title" id="calendars-heading">
          Calendars
        </h2>
        <p className="section__hint">Where meetings come from</p>
      </div>
      {sources === null ? (
        error ? null : (
          <Checking label="Reading your calendar settings…" />
        )
      ) : (
        <Card flush>
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
        </Card>
      )}
      {error ? <InlineError error={error} /> : null}
    </section>
  );
}

/** The Calendar app (macOS): a switch, like the Dock setting. */
function CalendarAppRow({
  on,
  onSaved,
}: {
  on: boolean;
  onSaved: (sources: CalendarSources) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const id = useId();

  async function change(next: boolean) {
    setBusy(true);
    setError(null);
    try {
      onSaved(await setCalendarApp(next));
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Row>
        <RowLabel
          name={<label htmlFor={id}>{CALENDAR_APP_LABEL}</label>}
          detail="Every account already in Calendar.app on this Mac. No sign-in needed."
          mono={false}
        />
        <button
          type="button"
          role="switch"
          id={id}
          aria-checked={on}
          disabled={busy}
          onClick={() => void change(!on)}
          className={cn(
            "inline-flex h-(--control-h-small) w-[36px] shrink-0 cursor-default items-center rounded-capsule p-1",
            "border-[0.5px] [transition:background-color_var(--dur-fast)_var(--ease-out)]",
            "disabled:cursor-not-allowed disabled:opacity-40",
            "contrast-more:border contrast-more:border-separator-strong",
            on ? "border-transparent bg-success" : "border-rim bg-glass-sunken",
          )}
        >
          <span
            aria-hidden="true"
            className={cn(
              "size-6 rounded-capsule bg-on-accent shadow-raised",
              "[transition:translate_var(--dur-fast)_var(--ease-out)] motion-reduce:transition-none",
              on ? "translate-x-6" : "translate-x-0",
            )}
          />
        </button>
      </Row>
      {error ? (
        <div className="px-6 pb-5">
          <InlineError error={error} />
        </div>
      ) : null}
    </>
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
  const [busy, setBusy] = useState<"connect" | "disconnect" | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const name = PROVIDER_NAME[provider];

  async function connect() {
    setBusy("connect");
    setError(null);
    try {
      onAccount(await calendarConnect(provider));
    } catch (thrown) {
      setError(signInError(provider, toUiError(thrown)));
    } finally {
      setBusy(null);
    }
  }

  async function disconnect() {
    setBusy("disconnect");
    setError(null);
    try {
      onSources(await calendarDisconnect(provider));
      onAccount({ provider, account: null, state: "signed-out", remembered: true });
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setBusy(null);
    }
  }

  const state = account?.state ?? "signed-out";
  const connectButton = (label: string, tone: "primary" | "neutral") => (
    <Button size="small" tone={tone} disabled={busy !== null} onClick={() => void connect()}>
      {busy === "connect" ? WAITING_FOR_BROWSER : label}
    </Button>
  );
  const disconnectButton = (
    <Button size="small" tone="quiet" disabled={busy !== null} onClick={() => void disconnect()}>
      {busy === "disconnect" ? "Disconnecting…" : "Disconnect"}
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
        <RowLabel name={name} detail={detail} mono={false} />
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
