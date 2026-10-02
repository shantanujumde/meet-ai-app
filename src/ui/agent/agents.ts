/**
 * What the setup screen knows about each agent before detection answers.
 *
 * Detection takes seconds (a login shell, `--version`, an auth check), so the
 * two rows are drawn straight away from this list and filled in when it
 * returns. Rust's answer wins wherever it has one.
 */

import type { AgentChoice, AgentCli, AgentCliId } from "@/ipc/types";

const KNOWN: Record<AgentCliId, { name: string; provider: string }> = {
  "claude-code": { name: "Claude Code", provider: "Anthropic" },
  codex: { name: "Codex", provider: "OpenAI" },
};

/** The two agents meet-ai can run, in the order detection returns them. */
export const AGENT_IDS: AgentCliId[] = ["claude-code", "codex"];

/** The model Claude Code starts on when nothing else says otherwise. */
export const CLAUDE_DEFAULT_MODEL = "opus";

/** One agent's name and provider, and what detection found, once it has answered. */
export function agentInfo(id: AgentCliId, found: AgentCli[] | null) {
  const cli = found?.find((agent) => agent.id === id);
  return { name: cli?.name ?? KNOWN[id].name, provider: cli?.provider ?? KNOWN[id].provider, cli };
}

/**
 * Which detection a choice asks for. A picked path only changes what
 * detection finds for that one agent, so detection runs again only when this
 * changes, not on every pick.
 */
export function detectKey(choice: AgentChoice | null): string {
  if (!choice || choice.harness === "none" || !choice.binaryPath) return "auto";
  return `${choice.harness}:${choice.binaryPath}`;
}
