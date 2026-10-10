/**
 * The agent setup's state: what config.jsonc says, what detection found, and
 * saving a change.
 *
 * Every change saves at once. There is no Save button to forget, and the
 * onboarding step and Settings stay the same screen.
 *
 * Detection runs CLIs, so its answer is kept for the session (TUR-171): a
 * second visit shows it at once, and "Check again" asks anew. In Settings the
 * saved pick comes from the visit's snapshot.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { agentChoice, type SettingsSnapshot, saveAgentChoice } from "@/ipc/client";
import type { AgentChoice, AgentCli, AgentHarness, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { useSavedAgent } from "@/state/savedAgent";
import { rememberAgentChoice, sessionDetectAgents } from "@/state/session";
import { readOrThrow, useSnapshotLoad } from "../settings/snapshot";
import { detectKey } from "./agents";

/** What detection is asked when config.jsonc could not be read: no path for either agent. */
const NO_CHOICE: AgentChoice = { harness: "none", model: "", binaryPath: null };

const pickChoice = (snapshot: SettingsSnapshot) =>
  readOrThrow(snapshot.agentChoice, snapshot.agentError);

export function useAgentSetup() {
  // Null until config.jsonc answers, and while it names an agent meet-ai
  // does not know. Then no option is shown as picked.
  const [choice, setChoice] = useState<AgentChoice | null>(null);
  const [loadError, setLoadError] = useState<UiError | null>(null);
  const [saveError, setSaveError] = useState<UiError | null>(null);
  const [agents, setAgents] = useState<AgentCli[] | null>(null);
  const [detecting, setDetecting] = useState(true);
  const [detectError, setDetectError] = useState<UiError | null>(null);

  // The latest choice, for saves that start from it. State alone would be a
  // render behind when two changes land close together.
  const current = useRef<AgentChoice | null>(null);
  // Only the newest detection may write its answer. "Check again" pressed
  // twice must not let the slower, older run win.
  const detectRun = useRef(0);
  // The same for saves: a model typed then blurred, then a suggestion
  // clicked, are two saves in flight, and only the newest may land.
  const saveRun = useRef(0);
  // The last model each agent had, so None → Claude Code finds it again.
  const lastModel = useRef<Partial<Record<AgentHarness, string>>>({});

  const apply = useCallback((next: AgentChoice | null) => {
    current.current = next;
    if (next && next.harness !== "none") lastModel.current[next.harness] = next.model;
    setChoice(next);
  }, []);

  const detect = useCallback(async (basis: AgentChoice | null, fresh = false) => {
    const run = ++detectRun.current;
    setDetecting(true);
    try {
      const found = await sessionDetectAgents(basis ?? NO_CHOICE, fresh);
      if (run !== detectRun.current) return;
      setAgents(found);
      setDetectError(null);
    } catch (thrown) {
      if (run === detectRun.current) setDetectError(toUiError(thrown));
    } finally {
      if (run === detectRun.current) setDetecting(false);
    }
  }, []);

  const load = useSnapshotLoad(pickChoice, agentChoice);
  useEffect(() => {
    let live = true;
    load().then(
      (loaded) => {
        if (!live) return;
        apply(loaded);
        void detect(loaded);
      },
      (thrown: unknown) => {
        if (!live) return;
        // An unknown `agent.harness` lands here. The picker still works, and
        // picking an agent writes a good value over the bad one.
        setLoadError(toUiError(thrown));
        void detect(null);
      },
    );
    return () => {
      live = false;
    };
  }, [apply, detect, load]);

  const save = useCallback(
    async (next: AgentChoice) => {
      const run = ++saveRun.current;
      const previous = current.current;
      apply(next);
      setSaveError(null);
      try {
        const saved = await saveAgentChoice(next);
        // A newer save started meanwhile: its answer is the one to show.
        if (run !== saveRun.current) return;
        apply(saved);
        // Before Tracker hears of it, so it asks a new agent for its servers.
        rememberAgentChoice(saved, previous);
        // TUR-170: Tracker, on the same page, follows the saved agent.
        useSavedAgent.getState().publish(saved);
        setLoadError(null);
        if (detectKey(saved) !== detectKey(previous)) void detect(saved);
      } catch (thrown) {
        if (run !== saveRun.current) return;
        apply(previous);
        setSaveError(toUiError(thrown));
      }
    },
    [apply, detect],
  );

  /**
   * Pick an agent, or `none`. An agent starts on the model it last had
   * here, else Default (a blank model: the agent picks its own), and on its
   * own path. `none` keeps the model, and each agent's last one is
   * remembered, so None → Claude Code finds it again.
   */
  const pick = useCallback(
    (harness: AgentHarness) => {
      const was = current.current;
      if (was?.harness === harness) return;
      const model = harness === "none" ? (was?.model ?? "") : (lastModel.current[harness] ?? "");
      void save({ harness, model, binaryPath: null });
    },
    [save],
  );

  /** Save a model name. Blank means the CLI's own default. */
  const setModel = useCallback(
    (model: string) => {
      const was = current.current;
      const trimmed = model.trim();
      if (!was || was.model === trimmed) return;
      void save({ ...was, model: trimmed });
    },
    [save],
  );

  /** Save where the CLI is, or null to look it up automatically again. */
  const setBinaryPath = useCallback(
    (binaryPath: string | null) => {
      const was = current.current;
      if (!was || was.binaryPath === binaryPath) return;
      void save({ ...was, binaryPath });
    },
    [save],
  );

  const checkAgain = useCallback(() => void detect(current.current, true), [detect]);

  return {
    choice,
    loadError,
    saveError,
    agents,
    detecting,
    detectError,
    pick,
    setModel,
    setBinaryPath,
    checkAgain,
  };
}
