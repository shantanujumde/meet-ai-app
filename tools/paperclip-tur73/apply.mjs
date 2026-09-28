#!/usr/bin/env node
// TUR-73 — local fix for four task-watchdog defects in @paperclipai/server.
//
// Paperclip ships to this machine as a published npm package that npx unpacks
// into a content-addressed cache directory, so there is no upstream checkout to
// edit. This script rewrites the shipped `dist/` JavaScript in place. It is
// idempotent, it refuses to touch a version it was not written against, and it
// keeps a `.tur73.orig` backup of every file it edits so `--revert` can undo it.
//
// Re-run it after any Paperclip upgrade or `npx` cache eviction. See README.md.

import { copyFileSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

const TARGET_VERSION = "2026.916.1";
const MARKER = "TUR-73 local fix";

const mode = process.argv.includes("--revert")
  ? "revert"
  : process.argv.includes("--check")
    ? "check"
    : "apply";

/** Every edit is anchored on an exact snippet of the shipped build output. */
function edits() {
  return [
    // ---------------------------------------------------------------- defect 1
    // The run's stop fingerprint is pinned once into heartbeat_runs.context_snapshot
    // and never written back, so the run's own first write permanently invalidates
    // the guard for the rest of the run. Carry the run id and a self-write
    // signature on the scope so the guard can tell "I changed this" from
    // "somebody else changed this".
    {
      file: "dist/services/task-watchdog-scope.js",
      find: `    return {
        watchedIssueId: readString(taskWatchdog?.watchedIssueId) ?? readString(context?.watchedIssueId),
        stopFingerprint: readString(taskWatchdog?.stopFingerprint) ?? readString(context?.stopFingerprint),
    };
}`,
      replace: `    return {
        watchedIssueId: readString(taskWatchdog?.watchedIssueId) ?? readString(context?.watchedIssueId),
        stopFingerprint: readString(taskWatchdog?.stopFingerprint) ?? readString(context?.stopFingerprint),
        // ${MARKER}: the state this run itself last produced and reviewed.
        selfWriteSignature: readString(taskWatchdog?.selfWriteSignature),
    };
}`,
    },
    {
      file: "dist/services/task-watchdog-scope.js",
      find: `    return {
        kind: "watchdog",
        watchdogId: watchdog.id,
        companyId: watchdog.companyId,
        watchedIssueId: watchdog.issueId,
        watchdogIssueId: watchdog.watchdogIssueId ?? null,
        stopFingerprint: taskWatchdog.stopFingerprint,
    };
}`,
      replace: `    return {
        kind: "watchdog",
        watchdogId: watchdog.id,
        companyId: watchdog.companyId,
        watchedIssueId: watchdog.issueId,
        watchdogIssueId: watchdog.watchdogIssueId ?? null,
        stopFingerprint: taskWatchdog.stopFingerprint,
        // ${MARKER}
        runId: run.id,
        selfWriteSignature: taskWatchdog.selfWriteSignature ?? null,
    };
}`,
    },
    {
      file: "dist/services/task-watchdog-scope.js",
      find: `//# sourceMappingURL=task-watchdog-scope.js.map`,
      replace: `// ${MARKER} (defect 1) ------------------------------------------------------
// A fingerprint alone only describes the "stopped" verdict. To recognise state
// this run produced in any verdict, sign the whole classification: the state
// name, the stop fingerprint when there is one, and the issue-id lists the
// classifier reports. Two different worlds cannot share a signature, so an
// outside edit still fails the guard even after this run has written once.
export function taskWatchdogObservedSignature(classification) {
    if (!classification || typeof classification !== "object" || Array.isArray(classification))
        return null;
    const state = readString(classification.state);
    if (!state)
        return null;
    const parts = [state];
    const fingerprint = readString(classification.stopFingerprint);
    if (fingerprint)
        parts.push(fingerprint);
    for (const key of ["liveIssueIds", "pendingIssueIds", "includedIssueIds"]) {
        const value = classification[key];
        if (Array.isArray(value)) {
            const ids = value.filter((id) => typeof id === "string").sort();
            parts.push(\`\${key}=\${ids.join(",")}\`);
        }
    }
    return parts.join("|");
}

// Re-pin the run's own observation after one of its writes lands. Best effort:
// if this fails the run simply keeps its previous pin and the guard stays
// closed, which is the behaviour without this patch.
export async function repinTaskWatchdogSelfWriteSignature(db, scope, signature) {
    if (!scope || scope.kind !== "watchdog" || !scope.runId || !signature)
        return;
    const run = await db
        .select({ contextSnapshot: heartbeatRuns.contextSnapshot })
        .from(heartbeatRuns)
        .where(eq(heartbeatRuns.id, scope.runId))
        .then((rows) => rows[0] ?? null);
    if (!run)
        return;
    const context = isPlainRecord(run.contextSnapshot) ? run.contextSnapshot : {};
    const taskWatchdog = isPlainRecord(context.taskWatchdog) ? context.taskWatchdog : {};
    if (taskWatchdog.selfWriteSignature === signature)
        return;
    await db
        .update(heartbeatRuns)
        .set({
        contextSnapshot: {
            ...context,
            taskWatchdog: { ...taskWatchdog, selfWriteSignature: signature },
        },
    })
        .where(eq(heartbeatRuns.id, scope.runId));
}
//# sourceMappingURL=task-watchdog-scope.js.map`,
    },
    {
      file: "dist/routes/issues.js",
      find: `import { TASK_WATCHDOG_ORIGIN_KIND, resolveTaskWatchdogMutationScope, taskWatchdogScopeAllowsIssueMutation, } from "../services/task-watchdog-scope.js";`,
      replace: `import { TASK_WATCHDOG_ORIGIN_KIND, resolveTaskWatchdogMutationScope, taskWatchdogScopeAllowsIssueMutation, repinTaskWatchdogSelfWriteSignature, taskWatchdogObservedSignature, } from "../services/task-watchdog-scope.js";`,
    },
    {
      file: "dist/routes/issues.js",
      find: `    async function assertFreshTaskWatchdogSourceMutation(res, scope, issue) {
        if (scope.kind !== "watchdog")
            return true;
        if (scope.watchdogIssueId && issue.id === scope.watchdogIssueId)
            return true;
        const revalidated = await taskWatchdogsSvc.revalidateMutationScope(scope);
        if (revalidated.allowed)
            return true;`,
      replace: `    // ${MARKER} (defect 1): arm a one-shot re-pin. When the response this guard
    // cleared finishes 2xx, recompute the subtree classification and store its
    // signature on the run row, so the run's next write is compared against the
    // state the run itself just produced instead of the now-stale original pin.
    const TASK_WATCHDOG_REPIN_ARMED = Symbol.for("paperclip.tur73.taskWatchdogRepinArmed");
    function armTaskWatchdogSelfWriteRepin(res, scope) {
        if (!res || typeof res.on !== "function" || res[TASK_WATCHDOG_REPIN_ARMED])
            return;
        res[TASK_WATCHDOG_REPIN_ARMED] = true;
        res.on("finish", () => {
            if (res.statusCode < 200 || res.statusCode >= 300)
                return;
            void (async () => {
                try {
                    const after = await taskWatchdogsSvc.revalidateMutationScope(scope);
                    const signature = taskWatchdogObservedSignature(after.classification);
                    if (signature)
                        await repinTaskWatchdogSelfWriteSignature(db, scope, signature);
                }
                catch {
                    // Leaving the old pin in place is the pre-patch behaviour.
                }
            })();
        });
    }
    async function taskWatchdogMutationFreshness(res, scope) {
        const revalidated = await taskWatchdogsSvc.revalidateMutationScope(scope);
        if (revalidated.allowed) {
            armTaskWatchdogSelfWriteRepin(res, scope);
            return revalidated;
        }
        const observed = taskWatchdogObservedSignature(revalidated.classification);
        if (observed && scope.selfWriteSignature && observed === scope.selfWriteSignature) {
            armTaskWatchdogSelfWriteRepin(res, scope);
            return { ...revalidated, allowed: true, selfWrite: true };
        }
        return revalidated;
    }
    async function assertFreshTaskWatchdogSourceMutation(res, scope, issue) {
        if (scope.kind !== "watchdog")
            return true;
        if (scope.watchdogIssueId && issue.id === scope.watchdogIssueId)
            return true;
        const revalidated = await taskWatchdogMutationFreshness(res, scope);
        if (revalidated.allowed)
            return true;`,
    },
    {
      file: "dist/routes/issues.js",
      find: `            const revalidated = await taskWatchdogsSvc.revalidateMutationScope(watchdogScope);
            if (!revalidated.allowed) {
                return denyIssueThreadInteractionResolution(res, {
                    status: 403,
                    code: "interaction_scope_denied",`,
      replace: `            const revalidated = await taskWatchdogMutationFreshness(res, watchdogScope); // ${MARKER}
            if (!revalidated.allowed) {
                return denyIssueThreadInteractionResolution(res, {
                    status: 403,
                    code: "interaction_scope_denied",`,
    },
    {
      file: "dist/routes/issues.js",
      find: `                    const revalidated = await taskWatchdogsSvc.revalidateMutationScope(watchdogScope);
                    if (!revalidated.allowed) {`,
      replace: `                    const revalidated = await taskWatchdogMutationFreshness(res, watchdogScope); // ${MARKER}
                    if (!revalidated.allowed) {`,
    },

    // ---------------------------------------------------------------- defect 2
    // Watchdog follow-ups are serialized behind "the current open child", but the
    // query filtered only on parent + open status. Any unrelated sibling — a
    // manual issue parked in in_review waiting on a human — became the anchor,
    // and every follow-up was born blocked behind it. Restrict the anchor to
    // children this watchdog agent actually created.
    {
      file: "dist/routes/issues.js",
      find: `    async function findCurrentSerializedWatchdogChild(parent) {
        const children = await db
            .select({
            id: issueRows.id,
            status: issueRows.status,
        })
            .from(issueRows)
            .where(and(eq(issueRows.companyId, parent.companyId), eq(issueRows.parentId, parent.id), inArray(issueRows.status, [
            "todo",
            "in_progress",
            "in_review",
            "blocked",
        ]), isNull(issueRows.hiddenAt)))`,
      replace: `    async function findCurrentSerializedWatchdogChild(parent, followUpAuthorAgentId = null) {
        // ${MARKER} (defect 2): only chain behind this watchdog's own follow-ups.
        const authorFilter = followUpAuthorAgentId
            ? [eq(issueRows.createdByAgentId, followUpAuthorAgentId)]
            : [];
        const children = await db
            .select({
            id: issueRows.id,
            status: issueRows.status,
        })
            .from(issueRows)
            .where(and(eq(issueRows.companyId, parent.companyId), eq(issueRows.parentId, parent.id), inArray(issueRows.status, [
            "todo",
            "in_progress",
            "in_review",
            "blocked",
        ]), isNull(issueRows.hiddenAt), ...authorFilter))`,
    },
    {
      file: "dist/routes/issues.js",
      find: `    async function resolveWatchdogFollowUpSerializationContext(req, parent) {
        if (parent.originKind === TASK_WATCHDOG_ORIGIN_KIND) {
            return {
                enabled: true,
                watchdogParentIssueId: parent.id,
            };
        }
        if (req.actor.type !== "agent")
            return null;
        const scope = await resolveTaskWatchdogMutationScope(db, req.actor);
        if (scope.kind !== "watchdog")
            return null;
        return {
            enabled: true,
            watchdogParentIssueId: scope.watchdogIssueId,
        };
    }`,
      replace: `    async function resolveWatchdogFollowUpSerializationContext(req, parent) {
        if (parent.originKind === TASK_WATCHDOG_ORIGIN_KIND) {
            // Every child of a watchdog issue is a follow-up by construction, so
            // no author filter is needed to identify the serialization anchor.
            return {
                enabled: true,
                watchdogParentIssueId: parent.id,
                followUpAuthorAgentId: null,
            };
        }
        if (req.actor.type !== "agent")
            return null;
        const scope = await resolveTaskWatchdogMutationScope(db, req.actor);
        if (scope.kind !== "watchdog")
            return null;
        return {
            enabled: true,
            watchdogParentIssueId: scope.watchdogIssueId,
            // ${MARKER} (defect 2): under a watched issue the siblings are ordinary
            // work, so the anchor must be one of this watchdog's own follow-ups.
            followUpAuthorAgentId: typeof req.actor.agentId === "string" ? req.actor.agentId : null,
        };
    }`,
    },
    {
      file: "dist/routes/issues.js",
      find: `        const currentSerializedChild = serializationContext
            ? await findCurrentSerializedWatchdogChild(parent)
            : null;`,
      replace: `        const currentSerializedChild = serializationContext
            ? await findCurrentSerializedWatchdogChild(parent, serializationContext.followUpAuthorAgentId ?? null)
            : null;`,
    },
    {
      file: "dist/routes/issues.js",
      find: `        const existingSerializedChild = serializationContext
            ? await findCurrentSerializedWatchdogChild(sourceIssue)
            : null;`,
      replace: `        const existingSerializedChild = serializationContext
            ? await findCurrentSerializedWatchdogChild(sourceIssue, serializationContext.followUpAuthorAgentId ?? null)
            : null;`,
    },

    // ---------------------------------------------------------------- defect 3
    // The published PATCH schema lists "board" as a valid unblock owner and the
    // runtime rejected it for every agent actor, so the one honest disposition
    // for work that needs a human was the one an agent could not record. Naming
    // a specific *user* stays restricted — that is a real delegation.
    {
      file: "dist/routes/issues.js",
      find: `            const owner = descriptor.owner;
            if (req.actor.type === "agent" &&
                (owner === "board" || "userId" in owner)) {
                throw forbidden("Agents may only name themselves as an unblock owner");
            }`,
      replace: `            const owner = descriptor.owner;
            // ${MARKER} (defect 3): "board" is a published owner value and is the
            // only honest disposition for work that genuinely needs a human.
            if (req.actor.type === "agent" &&
                owner !== "board" &&
                "userId" in owner) {
                throw forbidden("Agents may not name another user as an unblock owner");
            }`,
    },

    // ---------------------------------------------------------------- defect 4
    // Clearing a blocker and moving off blocked in one PATCH was rejected,
    // because readiness was read from the stored blocker set and ignored the
    // blockedByIssueIds the same request was sending. Two other call sites in
    // this file already prefer the requested set; this one did not.
    {
      file: "dist/routes/issues.js",
      find: `        const hasUnresolvedFirstClassBlockers = isBlocked && effectiveMoveToTodoRequested
            ? (await svc.getDependencyReadiness(existing.id))
                .unresolvedBlockerCount > 0
            : false;
        if (resumeRequested === true &&
            isBlocked &&
            hasUnresolvedFirstClassBlockers) {`,
      replace: `        // ${MARKER} (defect 4): a request that also rewrites blockedByIssueIds is
        // asking about the blockers it is setting, not the ones it is clearing.
        const requestedBlockerIdsForMoveToTodo = Array.isArray(req.body.blockedByIssueIds)
            ? [...new Set(req.body.blockedByIssueIds)]
            : null;
        const hasUnresolvedFirstClassBlockers = isBlocked && effectiveMoveToTodoRequested
            ? requestedBlockerIdsForMoveToTodo
                ? requestedBlockerIdsForMoveToTodo.length > 0 &&
                    (await db
                        .select({ id: issueRows.id })
                        .from(issueRows)
                        .where(and(eq(issueRows.companyId, existing.companyId), inArray(issueRows.id, requestedBlockerIdsForMoveToTodo), notInArray(issueRows.status, ["done", "cancelled"])))
                        .limit(1)
                        .then((rows) => rows.length > 0))
                : (await svc.getDependencyReadiness(existing.id))
                    .unresolvedBlockerCount > 0
            : false;
        if (resumeRequested === true &&
            isBlocked &&
            hasUnresolvedFirstClassBlockers) {`,
    },

    // ------------------------------------------------------- defect 4, part 2
    // The edit above only covers the explicit-resume branch. A plain agent
    // PATCH moving an issue off `blocked` takes a different route: it needs
    // "resume authority", which calls assertExplicitResumeIntentAllowed, and
    // that runs its own readiness check against the stored blocker set. So
    // {blockedByIssueIds: [], status: "todo"} still 409'd on the live server.
    // Found by running the check against a genuinely blocked issue.
    {
      file: "dist/routes/issues.js",
      // The bare `if (issue.status === "blocked")` opener also appears in the
      // recovery-action staleness check, so the anchor runs on to the error
      // string, which is unique to this one.
      find: `        if (issue.status === "blocked") {
            const readiness = await svc.getDependencyReadiness(issue.id);
            if (readiness.unresolvedBlockerCount > 0) {
                res.status(409).json({
                    error: "Issue follow-up blocked by unresolved blockers",`,
      replace: `        if (issue.status === "blocked") {
            // ${MARKER} (defect 4, part 2): when the same request rewrites the
            // blocker set, judge readiness against the set being requested.
            // Absent that field this is the stored set, exactly as before.
            const requestedBlockerIds = Array.isArray(req.body?.blockedByIssueIds)
                ? [...new Set(req.body.blockedByIssueIds)]
                : null;
            const readiness = requestedBlockerIds
                ? await (async () => {
                    if (requestedBlockerIds.length === 0) {
                        return { unresolvedBlockerCount: 0, unresolvedBlockerIssueIds: [] };
                    }
                    const open = await db
                        .select({ id: issueRows.id })
                        .from(issueRows)
                        .where(and(eq(issueRows.companyId, issue.companyId), inArray(issueRows.id, requestedBlockerIds), notInArray(issueRows.status, ["done", "cancelled"])));
                    return {
                        unresolvedBlockerCount: open.length,
                        unresolvedBlockerIssueIds: open.map((row) => row.id),
                    };
                })()
                : await svc.getDependencyReadiness(issue.id);
            if (readiness.unresolvedBlockerCount > 0) {
                res.status(409).json({
                    error: "Issue follow-up blocked by unresolved blockers",`,
    },
  ];
}

async function findInstalls() {
  // A Paperclip agent run sets HOME to a sandbox directory, so os.homedir() is
  // not the home that owns the npx cache. Try every home we can name.
  const explicit = process.argv
    .filter((arg) => arg.startsWith("--npx-root="))
    .map((arg) => arg.slice("--npx-root=".length));
  // In some runs *every* home we are handed is a sandbox temp dir and the real
  // npx cache is invisible, so also derive the login home from the workspace
  // path (/Users/<name>/... on darwin, /home/<name>/... elsewhere).
  const homeFromWorkspace = (() => {
    const cwd = process.env.PAPERCLIP_WORKSPACE_CWD ?? process.cwd();
    const match = /^((?:\/Users|\/home)\/[^/]+)(?:\/|$)/.exec(cwd);
    return match ? match[1] : null;
  })();
  const homes = [
    process.env.PAPERCLIP_GITHUB_HOST_HOME,
    homedir(),
    process.env.HOME,
    homeFromWorkspace,
  ].filter((value) => typeof value === "string" && value.length > 0);
  const roots = [
    ...new Set([...explicit, ...homes.map((home) => path.join(home, ".npm", "_npx"))]),
  ];
  const found = [];
  for (const root of roots) {
    if (!existsSync(root)) continue;
    let entries;
    try {
      entries = await readdir(root, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const pkg = path.join(root, entry.name, "node_modules", "@paperclipai", "server");
      if (existsSync(path.join(pkg, "package.json"))) found.push(pkg);
    }
  }
  return found.sort();
}

function versionOf(pkgRoot) {
  try {
    return JSON.parse(readFileSync(path.join(pkgRoot, "package.json"), "utf8")).version;
  } catch {
    return null;
  }
}

function run(pkgRoot) {
  const version = versionOf(pkgRoot);
  if (version !== TARGET_VERSION) {
    return { pkgRoot, version, status: "skipped-version" };
  }

  const results = [];
  for (const edit of edits()) {
    const file = path.join(pkgRoot, edit.file);
    const backup = `${file}.tur73.orig`;
    if (!existsSync(file)) {
      results.push({ edit: edit.file, status: "missing-file" });
      continue;
    }
    const source = readFileSync(file, "utf8");

    if (mode === "revert") {
      if (existsSync(backup)) {
        copyFileSync(backup, file);
        rmSync(backup);
        results.push({ edit: edit.file, status: "reverted" });
      } else {
        results.push({ edit: edit.file, status: "no-backup" });
      }
      continue;
    }

    if (source.includes(edit.replace)) {
      results.push({ edit: edit.file, status: "already-applied" });
      continue;
    }
    const hits = source.split(edit.find).length - 1;
    if (hits !== 1) {
      results.push({
        edit: edit.file,
        status: `anchor-${hits === 0 ? "missing" : `ambiguous(${hits})`}`,
      });
      continue;
    }
    if (mode === "check") {
      results.push({ edit: edit.file, status: "would-apply" });
      continue;
    }
    if (!existsSync(backup)) copyFileSync(file, backup);
    writeFileSync(file, source.replace(edit.find, edit.replace), "utf8");
    results.push({ edit: edit.file, status: "applied" });
  }
  return { pkgRoot, version, status: "processed", results };
}

const installs = await findInstalls();
if (installs.length === 0) {
  console.error("No @paperclipai/server install found under ~/.npm/_npx.");
  process.exit(1);
}

let failed = false;
for (const pkgRoot of installs) {
  const outcome = run(pkgRoot);
  console.log(`\n${pkgRoot}  (v${outcome.version ?? "unknown"})`);
  if (outcome.status === "skipped-version") {
    console.log(`  skipped: patch targets v${TARGET_VERSION}`);
    continue;
  }
  const counts = new Map();
  for (const r of outcome.results) {
    counts.set(r.status, (counts.get(r.status) ?? 0) + 1);
    if (r.status.startsWith("anchor-") || r.status === "missing-file") {
      failed = true;
      console.log(`  ${r.status.padEnd(20)} ${r.edit}`);
    }
  }
  for (const [status, count] of [...counts].sort()) {
    if (status.startsWith("anchor-") || status === "missing-file") continue;
    console.log(`  ${status.padEnd(20)} ${count}`);
  }
}

console.log(
  failed
    ? "\nSome edits did not apply. The shipped build probably changed; re-derive the anchors."
    : `\nDone (${mode}). Restart the Paperclip server for changes to take effect.`,
);
process.exit(failed ? 1 : 0);
