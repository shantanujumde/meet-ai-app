/**
 * Cancel on the agent Test screen (TUR-168). Stops the Test runs going,
 * which kills their CLIs; the pending `testAgent` then rejects with
 * `agent-cancelled`. Re-exported from `./client`; import from there.
 */

import { commands } from "./bindings";
import { call } from "./client";

/** Stop the agent Test. A no-op when none is running. */
export async function cancelAgentTest(): Promise<void> {
  await call(() => commands.cancelAgentTest());
}
