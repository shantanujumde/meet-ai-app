import { useEffect, useState } from "react";
import { agentChoice, detectAgents } from "@/ipc/client";

/**
 * Whether the agent CLI the user picked is installed (A11's fallback).
 *
 * Asks only when `ask` is true, i.e. an agent is set up, so a Mac with no agent
 * never spawns the detection. The button this feeds is an extra, so the answer
 * is `true` (CLI found) while the question is pending and on any error: a
 * failed check must not put Copy prompt in front of a working setup.
 */
export function useCliFound(ask: boolean): boolean {
  const [cliFound, setCliFound] = useState(true);
  useEffect(() => {
    if (!ask) return;
    let current = true;
    (async () => {
      const choice = await agentChoice();
      const found = await detectAgents(choice);
      return found.find((cli) => cli.id === choice.harness)?.state !== "missing";
    })().then(
      (answer) => {
        if (current) setCliFound(answer);
      },
      () => {
        if (current) setCliFound(true);
      },
    );
    return () => {
      current = false;
    };
  }, [ask]);
  return cliFound;
}
