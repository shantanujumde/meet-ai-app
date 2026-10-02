/**
 * When a meeting offers "Copy prompt" instead of having the app run an agent
 * (A11's fallback).
 *
 * Two reasons, either one enough: the user set no agent up (`agent.harness`
 * is `none`), or they chose one whose CLI cannot be found on this Mac.
 * `cliFound` defaults to true because nothing detects the CLI yet; agent CLI
 * detection (TUR-6, TUR-10) is what will pass it.
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
