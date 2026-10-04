/**
 * The Test button: a 3-line sample transcript run through the real notes
 * run, end to end, with the picked agent and model.
 *
 * It is the one way to know setup worked before a real meeting depends on
 * it, so it shows what came back — not just "OK" — and the agent's own
 * reason when it failed.
 */

import { FlaskConical } from "lucide-react";
import { type ReactNode, useState } from "react";
import { testAgent } from "@/ipc/client";
import type { AgentChoice, AgentTestResult, UiError } from "@/ipc/types";
import { toUiError } from "@/ipc/types";
import { Button, Row, RowLabel, rowDetailVariants } from "@/ui/primitives";
import { Checking, ErrorState } from "@/ui/states";

export function AgentTest({
  choice,
  name,
  blocked,
}: {
  choice: AgentChoice;
  /** The picked agent's name. */
  name: string;
  /** Why the test cannot run right now, or null when it can. */
  blocked: string | null;
}) {
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<AgentTestResult | null>(null);
  const [error, setError] = useState<UiError | null>(null);

  async function run() {
    setRunning(true);
    setResult(null);
    setError(null);
    try {
      setResult(await testAgent(choice));
    } catch (thrown) {
      setError(toUiError(thrown));
    } finally {
      setRunning(false);
    }
  }

  return (
    <Row stacked>
      <div className="flex justify-between gap-5">
        <RowLabel
          icon={FlaskConical}
          name="Test"
          detail={
            blocked ?? `Runs a 3-line sample meeting through ${name} and shows the notes it writes.`
          }
          mono={false}
        />
        <Button
          size="small"
          icon={FlaskConical}
          className="shrink-0"
          disabled={blocked !== null || running}
          onClick={() => void run()}
        >
          Test
        </Button>
      </div>
      {running ? <Checking label="Testing… this can take up to a minute." /> : null}
      {result ? <TestResult result={result} name={name} /> : null}
      {error ? <ErrorState error={error} /> : null}
    </Row>
  );
}

function TestResult({ result, name }: { result: AgentTestResult; name: string }) {
  const seconds = Math.max(1, Math.round(result.seconds));
  return (
    <div className="flex flex-col gap-3 rounded-control bg-glass-sunken px-5 py-4" role="status">
      <p className="text-body font-medium text-success">
        It works. {name} wrote these notes in {seconds} {seconds === 1 ? "second" : "seconds"}.
      </p>
      <p className="text-callout text-fg-primary">{result.summary}</p>
      <ResultList title="Tasks">
        {result.tasks.map((task) => (
          <li key={task.title}>
            {task.title}
            {task.owner ? ` · ${task.owner}` : ""}
            {task.due ? ` · due ${task.due}` : ""}
          </li>
        ))}
      </ResultList>
      <ResultList title="Decisions">
        {result.decisions.map((decision) => (
          <li key={decision}>{decision}</li>
        ))}
      </ResultList>
      <ResultList title="Open questions">
        {result.openQuestions.map((question) => (
          <li key={question}>{question}</li>
        ))}
      </ResultList>
    </div>
  );
}

/** A labelled list in the result. Left out when the run found nothing for it. */
function ResultList({ title, children }: { title: string; children: ReactNode[] }) {
  if (children.length === 0) return null;
  return (
    <div className="flex flex-col gap-1">
      <span className={rowDetailVariants({ mono: false })}>{title}</span>
      <ul className="list-disc pl-6 text-footnote text-fg-secondary">{children}</ul>
    </div>
  );
}
