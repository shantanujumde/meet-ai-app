/**
 * When a meeting offers "Copy prompt" instead of having the app run an agent
 * (A11's fallback).
 *
 * Two reasons, either one enough: the user set no agent up (`agent.harness`
 * is `none`), or they chose one whose CLI cannot be found on this Mac.
 * `cliFound` defaults to true so callers that have not detected the CLI keep
 * the button hidden; `useCliFound` passes the real answer.
 */
export function showsCopyPrompt({
  harnessIsNone,
  cliFound = true,
}: {
  harnessIsNone: boolean;
  cliFound?: boolean;
}): boolean {
  return harnessIsNone || !cliFound;
}
