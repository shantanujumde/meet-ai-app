/**
 * The agent setup's state: what config.jsonc says, what detection found, and
 * saving a change.
 *
 * Every change saves at once. There is no Save button to forget, and the
 * onboarding step and Settings stay the same screen.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { agentChoice, detectAgents, saveAgentChoice } from "@/ipc/client";
import type { AgentChoice, AgentCli, AgentHarness, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { detectKey } from "./agents";

/** What detection is asked when config.jsonc could not be read: no path for either agent. */
const NO_CHOICE: AgentChoice = { harness: "none", model: "", binaryPath: null };

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

  const apply = useCallback((next: AgentChoice | null) => {
    current.current = next;
    setChoice(next);
  }, []);

  const detect = useCallback(async (basis: AgentChoice | null) => {
    const run = ++detectRun.current;
    setDetecting(true);
    try {
      const found = await detectAgents(basis ?? NO_CHOICE);
      if (run !== detectRun.current) return;
      setAgents(found);
      setDetectError(null);
    } catch (thrown) {
      if (run === detectRun.current) setDetectError(toUiError(thrown));
    } finally {
      if (run === detectRun.current) setDetecting(false);
    }
  }, []);

  useEffect(() => {
    let live = true;
    agentChoice().then(
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
  }, [apply, detect]);

  const save = useCallback(
    async (next: AgentChoice) => {
      const previous = current.current;
      apply(next);
      setSaveError(null);
      try {
        const saved = await saveAgentChoice(next);
        apply(saved);
        setLoadError(null);
        if (detectKey(saved) !== detectKey(previous)) void detect(saved);
      } catch (thrown) {
        apply(previous);
        setSaveError(toUiError(thrown));
      }
    },
    [apply, detect],
  );

  /**
   * Pick an agent, or `none`. A new agent starts on Default (a blank model:
   * the agent picks its own) and its own path. `none` keeps the model, so
   * switching back finds it.
   */
  const pick = useCallback(
    (harness: AgentHarness) => {
      const was = current.current;
      if (was?.harness === harness) return;
      const model = harness === "none" ? (was?.model ?? "") : "";
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

  const checkAgain = useCallback(() => void detect(current.current), [detect]);

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
