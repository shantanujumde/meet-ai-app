/**
 * The agent the user last saved in this window (TUR-170), for the parts of
 * Settings that depend on it without owning it.
 *
 * Tracker reads its agent once, with its own settings, so an agent picked
 * above it on the same page left it naming the old one, with the old one's
 * tips and server list. The agent setup publishes each save here, and
 * Tracker follows. Only saves are published, not the first read: Tracker
 * reads that itself, and a second `mcp list` on open would cost up to a
 * minute for nothing.
 */

import { create } from "zustand";
import type { AgentChoice, AgentHarness } from "@/ipc/types";

/** What decides which CLI runs: the agent and its path. Not the model. */
export type SavedAgent = { harness: AgentHarness; binaryPath: string | null };

type SavedAgentStore = {
  /** Null until an agent is saved in this window. */
  saved: SavedAgent | null;
  /** Publish a saved choice. A model-only change is not a change here. */
  publish: (choice: AgentChoice) => void;
};

export const useSavedAgent = create<SavedAgentStore>((set, get) => ({
  saved: null,
  publish({ harness, binaryPath }) {
    const was = get().saved;
    if (was?.harness === harness && was.binaryPath === binaryPath) return;
    set({ saved: { harness, binaryPath } });
  },
}));
