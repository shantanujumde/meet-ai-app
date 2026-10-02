/**
 * The agent card: which agent writes the notes after a call, which model it
 * runs, and a Test that proves it works (SPEC A11, the Setup row).
 *
 * Shared by Settings and onboarding's last step, like the engine card, so
 * the two cannot describe the same choice differently. Every change saves at
 * once — see {@link useAgentSetup}.
 *
 * Detection takes seconds, so both agent rows are drawn at once and only
 * their status fills in, the same way the engine row does.
 */

import { useId } from "react";
import type { AgentChoice, AgentCli } from "@/ipc/types";
import { Button, ButtonRow, Card, Prose } from "@/ui/primitives";
import { ErrorState } from "@/ui/states";
import { ModelField, PathField } from "./AgentDetails";
import { AgentOption, AgentStatus, agentDetail, SignInCommand } from "./AgentOption";
import { AgentTest } from "./AgentTest";
import { AGENT_IDS, agentInfo } from "./agents";
import { useAgentSetup } from "./useAgentSetup";

export function AgentSetup() {
  const group = useId();
  const setup = useAgentSetup();
  const { choice, agents, detecting } = setup;

  return (
    <section className="section" aria-labelledby={`${group}-heading`}>
      <div className="section__header">
        <h2 className="section__title" id={`${group}-heading`}>
          Notes
        </h2>
        <p className="section__hint">Written by your own agent, on your own account</p>
      </div>

      {setup.loadError ? (
        <>
          <ErrorState error={setup.loadError} />
          <Prose>Picking an agent below fixes this.</Prose>
        </>
      ) : null}

      <fieldset className="contents">
        <legend className="sr-only">Who writes your notes</legend>
        <Card flush>
          {AGENT_IDS.map((id) => {
            const { name, cli } = agentInfo(id, agents);
            return (
              <AgentOption
                key={id}
                group={group}
                value={id}
                name={name}
                detail={agentDetail(cli, detecting)}
                checked={choice?.harness === id}
                onPick={() => setup.pick(id)}
                status={<AgentStatus cli={cli} detecting={detecting} />}
              >
                {!detecting && cli?.state === "signed-out" ? (
                  <SignInCommand command={cli.signInCommand} />
                ) : null}
              </AgentOption>
            );
          })}
          <AgentOption
            group={group}
            value="none"
            name="None, I'll copy the prompt"
            detail="After each call, Copy prompt puts the notes prompt on the clipboard for any agent you like"
            checked={choice?.harness === "none"}
            onPick={() => setup.pick("none")}
          />
        </Card>
      </fieldset>

      <ButtonRow>
        <Button size="small" disabled={detecting} onClick={setup.checkAgain}>
          {detecting ? "Checking…" : "Check again"}
        </Button>
      </ButtonRow>

      {setup.detectError ? <ErrorState error={setup.detectError} /> : null}
      {setup.saveError ? <ErrorState error={setup.saveError} /> : null}

      {choice ? (
        <PickedAgent
          choice={choice}
          agents={agents}
          onModel={setup.setModel}
          onPath={setup.setBinaryPath}
        />
      ) : null}
    </section>
  );
}

/** The privacy sentence, then the picked agent's model, path and Test. */
function PickedAgent({
  choice,
  agents,
  onModel,
  onPath,
}: {
  choice: AgentChoice;
  agents: AgentCli[] | null;
  onModel: (model: string) => void;
  onPath: (binaryPath: string | null) => void;
}) {
  if (choice.harness === "none") {
    return (
      <>
        <Prose>
          Nothing is sent anywhere. meet-ai runs no agent; you paste the prompt into one yourself.
        </Prose>
        <Card flush key="none">
          <AgentTest
            choice={choice}
            name="the agent"
            blocked="Nothing to test: with no agent picked, nothing runs."
          />
        </Card>
      </>
    );
  }

  const { name, provider, cli } = agentInfo(choice.harness, agents);
  // Before detection answers there is nothing to say it cannot run, and the
  // run itself reports a missing CLI plainly.
  const blocked =
    cli && (cli.state === "missing" || !cli.canTest)
      ? `Install ${name}, or choose where it is above, to test it.`
      : null;

  return (
    <>
      <Prose>
        When a call ends, meet-ai sends the transcript — never the audio — to {provider} through
        your own {name} account. You can turn this off for any meeting.
      </Prose>
      {/* Keyed by agent so a test result for one is never shown under another. */}
      <Card flush key={choice.harness}>
        <ModelField choice={choice} name={name} cli={cli} onSave={onModel} />
        <PathField choice={choice} name={name} cli={cli} onSave={onPath} />
        <AgentTest choice={choice} name={name} blocked={blocked} />
      </Card>
    </>
  );
}
