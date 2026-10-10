import { describe, expect, test, vi } from "vitest";
import type { AgentChoice } from "@/ipc/types";
import { ipc } from "@/test/ipcMock";
import {
  rememberAgentChoice,
  rememberNotesAutoRun,
  rememberSnapshot,
  sessionAgentChoice,
  sessionCopyPromptFallback,
  sessionDetectAgents,
  sessionNotesAutoRun,
  sessionTrackerServers,
} from "./session";

vi.mock("@/ipc/client", async (importOriginal) =>
  (await import("@/test/ipcMock")).mockClient(await importOriginal()),
);

const CLAUDE: AgentChoice = { harness: "claude-code", model: "", binaryPath: null };
const CODEX: AgentChoice = { harness: "codex", model: "", binaryPath: null };
const FAILED = { domain: "app" as const, kind: "io", message: "No." };

describe("the session's kept answers (TUR-171)", () => {
  test("each is asked once, and two asks at once share the one call", async () => {
    await Promise.all([sessionAgentChoice(), sessionAgentChoice()]);
    await sessionAgentChoice();
    await sessionCopyPromptFallback();
    await sessionCopyPromptFallback();
    await sessionNotesAutoRun();
    await sessionNotesAutoRun();
    expect(ipc.agentChoice).toHaveBeenCalledTimes(1);
    expect(ipc.copyPromptFallback).toHaveBeenCalledTimes(1);
    expect(ipc.notesAutoRun).toHaveBeenCalledTimes(1);
  });

  test("the CLI checks run once until asked fresh", async () => {
    await sessionDetectAgents(CLAUDE);
    await sessionDetectAgents(CLAUDE);
    await sessionTrackerServers();
    await sessionTrackerServers();
    expect(ipc.detectAgents).toHaveBeenCalledTimes(1);
    expect(ipc.trackerServers).toHaveBeenCalledTimes(1);

    await sessionDetectAgents(CLAUDE, true);
    await sessionTrackerServers(true);
    expect(ipc.detectAgents).toHaveBeenCalledTimes(2);
    expect(ipc.trackerServers).toHaveBeenCalledTimes(2);
  });

  test("detection is kept per CLI path", async () => {
    await sessionDetectAgents(CLAUDE);
    await sessionDetectAgents({ ...CLAUDE, binaryPath: "/opt/claude" });
    expect(ipc.detectAgents).toHaveBeenCalledTimes(2);
  });

  test("a failed ask is not kept, so the next one tries again", async () => {
    ipc.trackerServers.mockRejectedValueOnce(FAILED);
    ipc.detectAgents.mockRejectedValueOnce(FAILED);
    await expect(sessionTrackerServers()).rejects.toEqual(FAILED);
    await expect(sessionDetectAgents(CLAUDE)).rejects.toEqual(FAILED);
    await expect(sessionTrackerServers()).resolves.toEqual([]);
    await expect(sessionDetectAgents(CLAUDE)).resolves.toBeTruthy();
    expect(ipc.trackerServers).toHaveBeenCalledTimes(2);
    expect(ipc.detectAgents).toHaveBeenCalledTimes(2);
  });

  test("a save updates the kept answers, and a new agent asks for its servers again", async () => {
    await sessionTrackerServers();
    rememberAgentChoice({ ...CLAUDE, model: "opus" }, CLAUDE);
    await sessionTrackerServers();
    expect(ipc.trackerServers).toHaveBeenCalledTimes(1);

    rememberAgentChoice({ harness: "none", model: "", binaryPath: null }, CLAUDE);
    await sessionTrackerServers();
    expect(ipc.trackerServers).toHaveBeenCalledTimes(2);
    await expect(sessionCopyPromptFallback()).resolves.toBe(true);
    await expect(sessionAgentChoice()).resolves.toMatchObject({ harness: "none" });

    rememberNotesAutoRun(false);
    await expect(sessionNotesAutoRun()).resolves.toBe(false);
    expect(ipc.agentChoice).not.toHaveBeenCalled();
    expect(ipc.copyPromptFallback).not.toHaveBeenCalled();
    expect(ipc.notesAutoRun).not.toHaveBeenCalled();
  });

  test("a Settings snapshot brings the meeting view's answers up to date", async () => {
    const snapshot = await ipc.settingsSnapshot();
    if (!snapshot) throw new Error("the mock always answers");
    rememberSnapshot({ ...snapshot, agentChoice: CODEX, notesAutoRun: false });
    await expect(sessionAgentChoice()).resolves.toEqual(CODEX);
    await expect(sessionCopyPromptFallback()).resolves.toBe(false);
    await expect(sessionNotesAutoRun()).resolves.toBe(false);
    expect(ipc.agentChoice).not.toHaveBeenCalled();
  });
});
