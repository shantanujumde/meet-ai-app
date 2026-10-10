/**
 * Answers the window keeps for the session instead of asking Rust again on
 * every visit (TUR-171).
 *
 * Two kinds:
 *
 * - The slow CLI checks: which agent CLIs are installed and signed in
 *   (`detectAgents`, a login shell and `--version` per CLI) and the agent's
 *   tracker servers (`trackerServers`, `claude mcp list`, up to a minute).
 *   Settings used to run both on every visit. Now they run once, again on
 *   "Check again", and after a save that changes what they depend on.
 * - The few `config.jsonc` answers the meeting view asks on every open: is an
 *   agent set up (`copyPromptFallback`), which one (`agentChoice`), and do
 *   notes start on their own (`notesAutoRun`). Settings is the only screen
 *   that changes them, and its saves (and its snapshot) update them here.
 *
 * A promise is kept, not only the answer, so two screens asking at once share
 * one call. A call that fails is forgotten, so the next ask tries again.
 */

import { create } from "zustand";
import {
  agentChoice,
  copyPromptFallback,
  detectAgents,
  notesAutoRun,
  type SettingsSnapshot,
  trackerServers,
} from "@/ipc/client";
import type { AgentChoice, AgentCli, TrackerServer } from "@/ipc/types";
import { detectKey } from "@/ui/agent/agents";

type SessionStore = {
  agentChoice: Promise<AgentChoice> | null;
  copyPromptFallback: Promise<boolean> | null;
  notesAutoRun: Promise<boolean> | null;
  /** Detection per {@link detectKey}: the CLI paths it looked at. */
  agents: Record<string, Promise<AgentCli[]>>;
  trackerServers: Promise<TrackerServer[]> | null;
};

export const useSessionStore = create<SessionStore>(() => ({
  agentChoice: null,
  copyPromptFallback: null,
  notesAutoRun: null,
  agents: {},
  trackerServers: null,
}));

type Single = "agentChoice" | "copyPromptFallback" | "notesAutoRun" | "trackerServers";

/** The kept answer for `key`, or `ask` now (and kept) when there is none or `fresh`. */
function kept<K extends Single>(
  key: K,
  ask: () => NonNullable<SessionStore[K]>,
  fresh = false,
): NonNullable<SessionStore[K]> {
  const held = useSessionStore.getState()[key];
  if (held && !fresh) return held as NonNullable<SessionStore[K]>;
  const asked = ask();
  useSessionStore.setState({ [key]: asked } as Partial<SessionStore>);
  (asked as Promise<unknown>).catch(() => {
    if (useSessionStore.getState()[key] === asked) {
      useSessionStore.setState({ [key]: null } as Partial<SessionStore>);
    }
  });
  return asked;
}

/** `agentChoice`, asked once per session. */
export function sessionAgentChoice(): Promise<AgentChoice> {
  return kept("agentChoice", agentChoice);
}

/** `copyPromptFallback` (no agent set up), asked once per session. */
export function sessionCopyPromptFallback(): Promise<boolean> {
  return kept("copyPromptFallback", copyPromptFallback);
}

/** `notesAutoRun`, asked once per session. */
export function sessionNotesAutoRun(): Promise<boolean> {
  return kept("notesAutoRun", notesAutoRun);
}

/** The agent's tracker servers; `fresh` for "Check again". */
export function sessionTrackerServers(fresh = false): Promise<TrackerServer[]> {
  return kept("trackerServers", trackerServers, fresh);
}

/** The agent CLIs as `choice` finds them; `fresh` for "Check again". */
export function sessionDetectAgents(choice: AgentChoice, fresh = false): Promise<AgentCli[]> {
  const key = detectKey(choice);
  const held = useSessionStore.getState().agents[key];
  if (held && !fresh) return held;
  const asked = detectAgents(choice);
  useSessionStore.setState((state) => ({ agents: { ...state.agents, [key]: asked } }));
  asked.catch(() => {
    if (useSessionStore.getState().agents[key] !== asked) return;
    useSessionStore.setState(({ agents: { [key]: _gone, ...rest } }) => ({ agents: rest }));
  });
  return asked;
}

/**
 * A saved agent pick. The tracker servers are the agent's own list, so a
 * save that changes the agent or its path asks for them again (the same
 * change `useSavedAgent` tells Tracker about).
 */
export function rememberAgentChoice(saved: AgentChoice, previous: AgentChoice | null): void {
  const sameCli = previous?.harness === saved.harness && previous.binaryPath === saved.binaryPath;
  useSessionStore.setState({
    agentChoice: Promise.resolve(saved),
    copyPromptFallback: Promise.resolve(saved.harness === "none"),
    ...(sameCli ? {} : { trackerServers: null }),
  });
}

/** A saved "When notes run". */
export function rememberNotesAutoRun(on: boolean): void {
  useSessionStore.setState({ notesAutoRun: Promise.resolve(on) });
}

/** What a fresh Settings snapshot read, so the meeting view follows it. */
export function rememberSnapshot(snapshot: SettingsSnapshot): void {
  if (snapshot.agentChoice) {
    const choice = snapshot.agentChoice;
    useSessionStore.setState({
      agentChoice: Promise.resolve(choice),
      copyPromptFallback: Promise.resolve(choice.harness === "none"),
    });
  }
  if (snapshot.notesAutoRun !== null) {
    useSessionStore.setState({ notesAutoRun: Promise.resolve(snapshot.notesAutoRun) });
  }
}
