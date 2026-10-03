/**
 * What the setup screen knows about each agent before detection answers.
 *
 * Detection takes seconds (a login shell, `--version`, an auth check), so the
 * two rows are drawn straight away from this list and filled in when it
 * returns. Rust's answer wins wherever it has one.
 */

import type { AgentChoice, AgentCli, AgentCliId, AgentModel } from "@/ipc/types";

const KNOWN: Record<AgentCliId, { name: string; provider: string }> = {
  "claude-code": { name: "Claude Code", provider: "Anthropic" },
  codex: { name: "Codex", provider: "OpenAI" },
};

/** The two agents meet-ai can run, in the order detection returns them. */
export const AGENT_IDS: AgentCliId[] = ["claude-code", "codex"];

/**
 * How many of an agent's models get a button of their own, after "Default".
 * The rest go in the dropdown. The list's order decides which: Sonnet and
 * Haiku for Claude Code, the top two Codex lists for Codex.
 */
export const MODEL_CHIPS = 2;

/** An agent's models split into the buttons and the dropdown. */
export function modelChoices(models: AgentModel[]): { chips: AgentModel[]; more: AgentModel[] } {
  return { chips: models.slice(0, MODEL_CHIPS), more: models.slice(MODEL_CHIPS) };
}

/** "Sonnet: balanced, good default for notes", or just the label. */
export function modelText(model: AgentModel): string {
  return model.note ? `${model.label}: ${model.note}` : model.label;
}

/**
 * What a blank model means, in words: "Default (Claude Code picks)", or
 * "Default (Claude Code picks, currently Sonnet)" when the agent's own
 * settings say which model that is.
 */
export function defaultModelText(name: string, cli: AgentCli | undefined): string {
  const current = cli?.cliDefault;
  if (!current) return `Default (${name} picks)`;
  const known = cli.models.find((model) => model.name === current);
  return `Default (${name} picks, currently ${known?.label ?? current})`;
}

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
