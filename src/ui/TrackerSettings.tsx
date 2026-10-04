/**
 * Settings → Tracker: where Sync creates issues, and through which of the
 * agent's MCP servers (TUR-11).
 *
 * The server list comes from `claude mcp list` / `codex mcp list`, which can
 * take up to a minute. So it loads on its own, in its own spot, and the rest
 * of the section is usable while it does. A name typed by hand works too, for
 * a server the list does not show.
 *
 * The app never holds a tracker token: the agent signs in to the tracker
 * itself, through its own MCP connection.
 */

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { setTracker, trackerServers, trackerSettings } from "@/ipc/client";
import type {
  Harness,
  McpStatus,
  TrackerSettings as Settings,
  Tracker,
  TrackerServer,
  UiError,
} from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Button, ButtonRow, Card, Prose } from "./primitives";
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

export function TrackerSettings() {
  const [saved, setSaved] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<UiError | null>(null);
  const [tracker, setTrackerChoice] = useState<Tracker>("linear");
  const [server, setServer] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<UiError | null>(null);
  const [justSaved, setJustSaved] = useState(false);

  useEffect(() => {
    let current = true;
    trackerSettings().then(
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
  }, []);

  const name = server.trim();
  const changed = saved === null || saved.tracker !== tracker || saved.trackerMcp !== name;
  const canSave = name.length > 0 && changed && !saving;

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

  const harness = saved?.harness ?? null;

  return (
    <section className="section" aria-labelledby="tracker-heading">
      <div className="section__header">
        <h2 className="section__title" id="tracker-heading">
          Tracker
        </h2>
        <p className="section__hint">Where Sync creates issues for your tasks</p>
      </div>
      <Card>
        <form className="flex flex-col gap-5" onSubmit={(event) => void save(event)}>
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
            value={server}
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
              {justSaved ? "Saved" : ""}
            </span>
          </ButtonRow>
        </form>
      </Card>
    </section>
  );
}

/** Which agent runs Sync, or that there is none to run it. */
function AgentLine({ harness }: { harness: Harness }) {
  if (harness === "none") {
    return (
      <p className="text-callout text-warning">
        No agent is set up. Sync needs Claude Code or Codex to create issues.
      </p>
    );
  }
  return (
    <p className="text-callout text-fg-secondary">
      Sync runs in <span className="font-medium text-fg-primary">{HARNESS_NAME[harness]}</span>.
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
function ServerPicker({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const [servers, setServers] = useState<TrackerServer[] | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [checking, setChecking] = useState(true);

  const check = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      setServers(await trackerServers());
    } catch (caught) {
      setError(toUiError(caught));
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    void check();
  }, [check]);

  const listed = servers?.some((each) => each.name === value.trim()) ?? false;

  return (
    <div className="flex flex-col gap-4">
      {servers && servers.length > 0 ? (
        <label className={LABEL}>
          Server
          <select
            className={FIELD}
            value={listed ? value.trim() : ""}
            onChange={(event) => {
              if (event.target.value) onChange(event.target.value);
            }}
          >
            <option value="">Pick a server…</option>
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
        <Button size="small" disabled={checking} onClick={() => void check()}>
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
