/**
 * Settings → Tracker: where tickets are sent, and through which of the
 * agent's MCP servers (TUR-11).
 *
 * The server list comes from `claude mcp list` / `codex mcp list`, which can
 * take up to a minute. So it loads on its own, in its own spot, and the rest
 * of the section is usable while it does. A name typed by hand works too, for
 * a server the list does not show.
 *
 * The app never holds a tracker token: the agent signs in to the tracker
 * itself, through its own MCP connection.
 *
 * Until the user saves, Rust answers with the shipped defaults and
 * `chosen: false`. Those count as not saved: Save stays on and says so, or
 * the defaults would look saved while no ticket is ever sent. A test ticket
 * saves nothing (A28): only Save does.
 */

import { ListTodo, RefreshCw, Send } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { type SettingsSnapshot, sendTestTicket, setTracker, trackerSettings } from "@/ipc/client";
import type {
  Harness,
  McpStatus,
  TrackerSettings as Settings,
  Tracker,
  TrackerServer,
  UiError,
} from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { type SavedAgent, useSavedAgent } from "@/state/savedAgent";
import { sessionTrackerServers } from "@/state/session";
import { Icon } from "./icons";
import { Button, ButtonRow, Prose } from "./primitives";
import { SettingsSection } from "./settings/SettingsSection";
import { readOrThrow, useSnapshotLoad } from "./settings/snapshot";
import { Checking, InlineError } from "./states";

const TRACKERS: { value: Tracker; label: string }[] = [
  { value: "linear", label: "Linear" },
  { value: "jira", label: "Jira" },
  { value: "github", label: "GitHub" },
];

/** A server's state in plain words. */
export const STATUS_WORDS: Record<McpStatus, string> = {
  connected: "Connected",
  needs_auth: "Needs sign-in",
  failed: "Could not connect",
  pending: "Waiting for approval",
  disabled: "Turned off",
  configured: "Set up",
  unknown: "Unknown",
};

const HARNESS_NAME: Record<Harness, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  none: "None",
};

const FIELD =
  "w-full rounded-control border-[0.5px] border-separator bg-glass-sunken px-4 py-3 text-body text-fg-primary";

const LABEL = "flex flex-col gap-2 text-callout text-fg-secondary";

const pick = (snapshot: SettingsSnapshot) => readOrThrow(snapshot.tracker, snapshot.trackerError);

export function TrackerSettings() {
  const [saved, setSaved] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<UiError | null>(null);
  const [tracker, setTrackerChoice] = useState<Tracker>("linear");
  const [server, setServer] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<UiError | null>(null);
  const [justSaved, setJustSaved] = useState(false);
  /** The values the last test ticket that got through checked. */
  const [passed, setPassed] = useState<{ tracker: Tracker; server: string } | null>(null);

  const load = useSnapshotLoad(pick, trackerSettings);

  useEffect(() => {
    let current = true;
    load().then(
      (settings) => {
        if (!current) return;
        setSaved(settings);
        setTrackerChoice(settings.tracker);
        setServer(settings.trackerMcp);
      },
      (caught) => {
        if (current) setLoadError(toUiError(caught));
      },
    );
    return () => {
      current = false;
    };
  }, [load]);

  const name = server.trim();
  const unsaved = name.length > 0 && !isSaved(saved, tracker, name);
  const canSave = unsaved && !saving;
  const testedOnScreen = passed?.tracker === tracker && passed.server === name;

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!canSave) return;
    setSaving(true);
    setSaveError(null);
    setJustSaved(false);
    try {
      const settings = await setTracker(tracker, name);
      setSaved(settings);
      setTrackerChoice(settings.tracker);
      setServer(settings.trackerMcp);
      setJustSaved(true);
    } catch (caught) {
      setSaveError(toUiError(caught));
    } finally {
      setSaving(false);
    }
  }

  // TUR-170: an agent saved since this section read its settings (above it,
  // on the same page) wins, so the line, the tip and the servers follow it.
  const agent = useSavedAgent((state) => state.saved);
  const harness = agent?.harness ?? saved?.harness ?? null;

  return (
    <SettingsSection title="Tracker" anchorId="tracker">
      <form className="flex flex-col gap-5 py-6" onSubmit={(event) => void save(event)}>
        <HowItWorks />
        {harness ? <AgentLine harness={harness} /> : null}
        {loadError ? <InlineError error={loadError} /> : null}

        <label className={LABEL}>
          Tracker
          <select
            className={FIELD}
            value={tracker}
            onChange={(event) => {
              setTrackerChoice(event.target.value as Tracker);
              setJustSaved(false);
            }}
          >
            {TRACKERS.map((each) => (
              <option key={each.value} value={each.value}>
                {each.label}
              </option>
            ))}
          </select>
        </label>

        <ServerPicker
          agent={agent}
          value={server}
          saved={saved?.chosen ? saved.trackerMcp : null}
          onChange={(value) => {
            setServer(value);
            setJustSaved(false);
          }}
        />

        <Prose>{tipFor(harness)}</Prose>
        <Prose>
          meet-ai never stores tracker tokens. The agent signs in to your tracker through its own
          connection.
        </Prose>

        {saveError ? <InlineError error={saveError} /> : null}
        <ButtonRow>
          <Button type="submit" disabled={!canSave}>
            {saving ? "Saving…" : "Save"}
          </Button>
          <span className="text-footnote text-fg-secondary" role="status" aria-live="polite">
            {saveNote(saving, unsaved, testedOnScreen, justSaved)}
          </span>
        </ButtonRow>
        <TestTicket
          tracker={tracker}
          server={name}
          onPassed={(checkedTracker, checkedServer) =>
            setPassed({ tracker: checkedTracker, server: checkedServer })
          }
        />
      </form>
    </SettingsSection>
  );
}

/** Whether `tracker` and `server` are what the user saved (not only the defaults). */
function isSaved(saved: Settings | null, tracker: Tracker, server: string): boolean {
  return saved?.chosen === true && saved.tracker === tracker && saved.trackerMcp === server;
}

/** The note beside Save. `tested`: a test ticket got through with the values on screen. */
function saveNote(saving: boolean, unsaved: boolean, tested: boolean, justSaved: boolean): string {
  if (saving) return "";
  if (unsaved) return tested ? "Test passed. Press Save to keep it." : "Not saved yet";
  return justSaved ? "Saved" : "";
}

/** What this card is for, and the three steps to make it work (TUR-113). */
function HowItWorks() {
  return (
    <div className="flex flex-col gap-3">
      <Prose>
        Pick where your tickets go: Linear, Jira or GitHub. meet-ai doesn't sign in to your tracker
        itself. Your agent (Claude Code or Codex) sends each ticket using one of its own connections
        (an MCP server).
      </Prose>
      <ol className="flex list-decimal flex-col gap-1 pl-8 text-callout text-fg-secondary">
        <li>Connect your tracker in your agent first, e.g. claude.ai Linear.</li>
        <li>Pick the tracker and that connection below.</li>
        <li>
          Press <span className="font-semibold text-fg-primary">Send a test ticket</span> to check
          it works.
        </li>
      </ol>
    </div>
  );
}

/**
 * Send a test ticket: the agent checks it reaches the tracker through the
 * connection on screen, saved or not. Read-only: nothing is created.
 * `onPassed` gets the values it checked, which may no longer be on screen.
 * It saves nothing.
 */
function TestTicket({
  tracker,
  server,
  onPassed,
}: {
  tracker: Tracker;
  server: string;
  onPassed: (tracker: Tracker, server: string) => void;
}) {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  async function check() {
    setChecking(true);
    setResult(null);
    setError(null);
    try {
      setResult((await sendTestTicket(tracker, server)).message);
      onPassed(tracker, server);
    } catch (caught) {
      setError(toUiError(caught));
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <ButtonRow>
        <Button
          size="small"
          icon={Send}
          disabled={checking || server.length === 0}
          onClick={() => void check()}
        >
          {checking ? "Sending a test ticket…" : "Send a test ticket"}
        </Button>
      </ButtonRow>
      <p className="text-footnote text-fg-secondary" role="status" aria-live="polite">
        {result ?? ""}
      </p>
      {error ? <InlineError error={error} /> : null}
    </div>
  );
}

/** Which agent sends tickets, or that there is none to send them. */
function AgentLine({ harness }: { harness: Harness }) {
  if (harness === "none") {
    return (
      <p className="flex items-center gap-3 text-callout text-warning">
        <Icon icon={ListTodo} />
        No agent is set up. Sending tickets needs Claude Code or Codex.
      </p>
    );
  }
  return (
    <p className="flex items-center gap-3 text-callout text-fg-secondary">
      <Icon icon={ListTodo} />
      <span>
        Tickets are sent by{" "}
        <span className="font-medium text-fg-primary">{HARNESS_NAME[harness]}</span>.
      </span>
    </p>
  );
}

function tipFor(harness: Harness | null): string {
  if (harness === "codex") {
    return "The agent runs in an empty folder, so it only loads servers (tracker connections, also called MCP servers) from your own Codex setup. Add one with `codex mcp add …`.";
  }
  return "The agent runs in an empty folder, so it only loads servers (tracker connections, also called MCP servers) added for your user. Add one with `claude mcp add --scope user …`.";
}

/**
 * The server: a list of what the agent reports, and a text field for any
 * other name. Both edit the same value.
 */
function ServerPicker({
  agent,
  value,
  saved,
  onChange,
}: {
  /** The agent saved in this window, if any: a new one is asked again. */
  agent: SavedAgent | null;
  value: string;
  /** The saved server name, or null when nothing is saved. */
  saved: string | null;
  onChange: (value: string) => void;
}) {
  const [servers, setServers] = useState<TrackerServer[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [checking, setChecking] = useState(true);

  // `claude mcp list` can take a minute, so its answer is kept for the
  // session (TUR-171); "Check again" asks anew.
  const check = useCallback(async (fresh = false) => {
    setChecking(true);
    setError(null);
    try {
      setServers(await sessionTrackerServers(fresh));
    } catch (caught) {
      setError(toUiError(caught));
    } finally {
      setChecking(false);
    }
  }, []);

  // Asked on open, and again when another agent is saved (TUR-170): the list
  // is that agent's (`claude mcp list` or `codex mcp list`). On open the
  // session's answer will do (TUR-171); a new agent always asks.
  const askedFor = useRef(agent);
  useEffect(() => {
    const changed = askedFor.current !== agent;
    askedFor.current = agent;
    void check(changed);
  }, [check, agent]);

  const name = value.trim();
  const listed = servers?.some((each) => each.name === name) ?? false;
  // A saved name the agent does not list still shows as picked. A name being
  // typed does not get an entry of its own.
  const unlisted = saved && !servers?.some((each) => each.name === saved) ? saved : null;

  return (
    <div className="flex flex-col gap-4">
      {servers && servers.length > 0 ? (
        <label className={LABEL}>
          Server
          <select
            className={FIELD}
            value={listed || name === unlisted ? name : ""}
            onChange={(event) => {
              if (event.target.value) onChange(event.target.value);
            }}
          >
            <option value="">Pick a server…</option>
            {unlisted ? (
              <option value={unlisted}>{`${unlisted} (not in your agent's list)`}</option>
            ) : null}
            {servers.map((each) => (
              <option key={each.name} value={each.name}>
                {`${each.name} (${STATUS_WORDS[each.status]})`}
              </option>
            ))}
          </select>
        </label>
      ) : null}

      <ButtonRow>
        {checking ? (
          <Checking label="Checking your agent's servers…" />
        ) : servers && servers.length === 0 ? (
          <span className="text-footnote text-fg-secondary">
            Your agent has no servers added yet.
          </span>
        ) : null}
        <Button size="small" icon={RefreshCw} disabled={checking} onClick={() => void check(true)}>
          Check again
        </Button>
      </ButtonRow>
      {error ? <InlineError error={error} /> : null}

      <label className={LABEL}>
        Server name
        <input
          className={FIELD}
          value={value}
          placeholder="claude.ai Linear"
          onChange={(event) => onChange(event.target.value)}
        />
      </label>
    </div>
  );
}
