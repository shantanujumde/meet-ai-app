/**
 * One choice in the agent list: a radio, the agent's name, and what
 * detection found — signed in, installed but signed out, or not found.
 *
 * A signed-out agent shows the exact command that signs it in, because
 * "sign in to Claude Code" is not instructions when the app cannot open a
 * Terminal for you.
 */

import { type ReactNode, useState } from "react";
import type { AgentCli } from "@/ipc/types";
import { copyText } from "@/lib/clipboard";
import { COPIED_RESET_MS } from "@/lib/constants";
import { Button, Pill, Row, RowValue, rowDetailVariants } from "@/ui/primitives";
import { Checking } from "@/ui/states";

export function AgentOption({
  group,
  value,
  name,
  detail,
  checked,
  onPick,
  status,
  children,
}: {
  /** The radio group's shared `name`. */
  group: string;
  value: string;
  name: string;
  detail: string;
  checked: boolean;
  onPick: () => void;
  /** The right-hand side: a pill, or "Checking…". */
  status?: ReactNode;
  /** More under the row, such as the sign-in command. */
  children?: ReactNode;
}) {
  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        <label className="flex min-w-0 items-start gap-4">
          <input
            type="radio"
            name={group}
            value={value}
            checked={checked}
            onChange={onPick}
            className="mt-1 size-4 shrink-0 accent-accent"
          />
          <span className="flex min-w-0 flex-col gap-1">
            <span className="text-body font-medium">{name}</span>
            <span className={rowDetailVariants({ mono: false })}>{detail}</span>
          </span>
        </label>
        {status ? <RowValue className="shrink-0">{status}</RowValue> : null}
      </div>
      {children}
    </Row>
  );
}

/** The pill for what detection found, or "Checking…" while it runs. */
export function AgentStatus({ cli, detecting }: { cli: AgentCli | undefined; detecting: boolean }) {
  if (detecting) return <Checking label="Checking…" />;
  if (!cli) return <Pill>Not checked</Pill>;
  if (cli.state === "ready") return <Pill tone="ok">Signed in</Pill>;
  if (cli.state === "signed-out") return <Pill tone="warn">Installed, not signed in</Pill>;
  return <Pill>Not found</Pill>;
}

/** The line under an agent's name: its version and where it is, or what is wrong. */
export function agentDetail(cli: AgentCli | undefined, detecting: boolean): string {
  if (!cli) return detecting ? "Looking for it on this Mac" : "Not checked yet";
  if (cli.state === "missing") return "Not installed, or not where meet-ai looked";
  const version = cli.version ? `Version ${cli.version}` : "Version unknown";
  return cli.path ? `${version} · ${cli.path}` : version;
}

/** The sign-in command, selectable and with a Copy button, and what to do with it. */
export function SignInCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await copyText(command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch {
      // The command is on screen and selectable, so a refused clipboard
      // costs one manual copy and is not worth an error.
      setCopied(false);
    }
  }

  return (
    <div className="flex flex-col gap-2 pl-8">
      <p className="text-footnote text-fg-secondary">Run this in Terminal, then check again:</p>
      <div className="flex items-center gap-4">
        <code className="min-w-0 flex-1 select-all wrap-anywhere rounded-control border-[0.5px] border-separator bg-glass-sunken px-4 py-2 font-mono text-caption1 text-fg-primary">
          {command}
        </code>
        <Button size="small" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
    </div>
  );
}
