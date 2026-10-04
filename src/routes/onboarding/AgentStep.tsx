/**
 * Onboarding step 5: which agent writes the notes after a call, and on which
 * model (SPEC A11, the Setup row).
 *
 * The card is Settings' own {@link AgentSetup}, so setup and Settings can
 * never describe the same choice differently.
 */

import { AgentSetup } from "@/ui/agent/AgentSetup";
import { Button, ButtonRow, Prose } from "@/ui/primitives";

export function AgentStep({ onFinish }: { onFinish: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title">Who writes your notes</h1>
      </header>
      <Prose>
        When a call ends, meet-ai gives the transcript to an agent you already use: Claude Code or
        Codex. It turns what the agent writes into notes and tasks. The agent uses your own account.
        meet-ai has no AI of its own. Pick one here, or copy the prompt by hand instead.
      </Prose>

      <AgentSetup />

      <ButtonRow>
        <Button tone="primary" onClick={onFinish}>
          Done
        </Button>
      </ButtonRow>
    </>
  );
}
