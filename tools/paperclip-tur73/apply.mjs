#!/usr/bin/env node
// TUR-73 — local fix for six task-watchdog defects in @paperclipai/server.
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
      // The defect-5 edit below rewrites text *inside* this block, so the block
      // as written here stops appearing verbatim once both are applied. Match on
      // a line no later edit touches instead, or a second run re-appends the
      // whole thing and the module ends up with two copies of each export.
      appliedMarker: `export async function repinTaskWatchdogSelfWriteSignature(db, scope, signature) {`,
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

    // ---------------------------------------------------------------- defect 5
    // The re-pin above only covers "stopped -> stopped, different fingerprint".
    // When the run's own write is the one that makes the subtree live again, the
    // verdict flips to live/pending_first_run and no signature captured on the
    // previous response can match: the scheduler's side effects land after the
    // response finishes, so the state the next write sees was never observed by
    // anyone. Name the issues the classifier blames for the flip instead, so the
    // guard can ask the only question that matters — did this run cause it?
    {
      file: "dist/services/task-watchdog-scope.js",
      find: `export function taskWatchdogObservedSignature(classification) {`,
      replace: `// ${MARKER} (defect 5): the issues the classifier blames for a non-stopped
// verdict. Returns null for verdicts with no attributable issue (already
// reviewed, not applicable), which keeps those on the strict path.
export function taskWatchdogRecoveryAttributableIssueIds(classification) {
    if (!isPlainRecord(classification))
        return null;
    const state = readString(classification.state);
    const key = state === "live"
        ? "liveIssueIds"
        : state === "pending_first_run"
            ? "pendingIssueIds"
            : null;
    if (!key || !Array.isArray(classification[key]))
        return null;
    const ids = classification[key].filter((id) => typeof id === "string" && id.length > 0);
    return ids.length > 0 ? ids : null;
}

export function taskWatchdogObservedSignature(classification) {`,
    },

    {
      file: "dist/routes/issues.js",
      find: `import { TASK_WATCHDOG_ORIGIN_KIND, resolveTaskWatchdogMutationScope, taskWatchdogScopeAllowsIssueMutation, } from "../services/task-watchdog-scope.js";`,
      replace: `import { TASK_WATCHDOG_ORIGIN_KIND, resolveTaskWatchdogMutationScope, taskWatchdogScopeAllowsIssueMutation, repinTaskWatchdogSelfWriteSignature, taskWatchdogObservedSignature, taskWatchdogRecoveryAttributableIssueIds, } from "../services/task-watchdog-scope.js";`,
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
    // ${MARKER} (defect 5): which issues has this run already written to? The
    // activity log records the run id on every issue mutation, so this is exact
    // provenance rather than a guess, and it does not depend on a re-pin having
    // won a race against the write's own downstream effects.
    async function taskWatchdogRunWrittenIssueIds(scope) {
        if (!scope?.runId)
            return new Set();
        const rows = await db
            .select({ entityId: activityLog.entityId })
            .from(activityLog)
            .where(and(eq(activityLog.runId, scope.runId), eq(activityLog.entityType, "issue")));
        return new Set(rows.map((row) => row.entityId).filter((id) => typeof id === "string"));
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
        // ${MARKER} (defect 5): the subtree is no longer stopped. Let the run
        // finish the writes that go with the recovery it just performed — but
        // only when every issue the classifier credits for that recovery is one
        // this run wrote to. A path someone else revived still closes the guard.
        const attributable = taskWatchdogRecoveryAttributableIssueIds(revalidated.classification);
        if (attributable) {
            const written = await taskWatchdogRunWrittenIssueIds(scope);
            if (written.size > 0 && attributable.every((issueId) => written.has(issueId))) {
                armTaskWatchdogSelfWriteRepin(res, scope);
                return { ...revalidated, allowed: true, selfWrite: true, selfRecovered: true };
            }
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

    // ---------------------------------------------------------------- defect 6
    // The freshness gate also covered comments, so once a watchdog restored a
    // live path it could no longer write any record to the tree it had just
    // repaired — including the summary comment its own mandate asks for. A
    // comment sets no status, no blocker and no assignee, so a stale review
    // cannot make one dangerous. The subtree scope check above still applies.
    {
      file: "dist/routes/issues.js",
      // The sibling call site is identical except for the "issue:mutate"
      // permission on the next line, so the anchor runs on to "issue:comment".
      find: `            return assertFreshTaskWatchdogSourceMutation(res, watchdogScope, issue);
        }
        const boundaryDecision = await decideIssueAccess(req, issue, "issue:comment");`,
      replace: `            // ${MARKER} (defect 6): comments are records, not state changes.
            return true;
        }
        const boundaryDecision = await decideIssueAccess(req, issue, "issue:comment");`,
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

    // ------------------------------------------------------- defect 2, part 2
    // The author filter above was not enough. A watchdog's own *review* issue is
    // also a child of the watched issue and is also created by the watchdog
    // agent, so it matched the filter and became the anchor — and a follow-up was
    // born blocked behind the very review issue that was creating it. Observed
    // live: TUR-140 born blocked behind TUR-136. A review issue is not a
    // follow-up. Exclude every task_watchdog-origin sibling.
    //
    // `origin_kind` is NOT NULL with default 'manual' (@paperclipai/db
    // schema/issues.js), so notInArray has no three-valued-logic trap here.
    // This only narrows the author-filtered branch: when the parent *is* the
    // watchdog issue the filter list is empty and behaviour is unchanged,
    // which is correct — there every child really is a follow-up.
    {
      file: "dist/routes/issues.js",
      find: `        const authorFilter = followUpAuthorAgentId
            ? [eq(issueRows.createdByAgentId, followUpAuthorAgentId)]
            : [];`,
      replace: `        const authorFilter = followUpAuthorAgentId
            ? [
                eq(issueRows.createdByAgentId, followUpAuthorAgentId),
                // ${MARKER} (defect 2, part 2): a watchdog review issue is a
                // same-agent child of the watched issue, but it is not a follow-up.
                notInArray(issueRows.originKind, [TASK_WATCHDOG_ORIGIN_KIND]),
            ]
            : [];`,
    },

    // ------------------------------------------------------- defect 2, part 3
    // Serialization wires its blocker edges *after* the row in the 201 body was
    // composed, so the response showed `blocks: []` on a child that had just been
    // made a blocker of its sibling. A caller that trusts the 201 — as the TUR-72
    // run did — cannot see the edge it just created, and only a later GET reveals
    // it. Re-read the row before answering, but only on the serialization path so
    // ordinary child creation keeps its single write.
    {
      file: "dist/routes/issues.js",
      find: `            currentChildIssueId: currentSerializedChild?.id ?? issue.id,
        });
        await queueTaskWatchdogEvaluation(issue, actor.runId);
        res.status(201).json(issue);`,
      replace: `            currentChildIssueId: currentSerializedChild?.id ?? issue.id,
        });
        await queueTaskWatchdogEvaluation(issue, actor.runId);
        // ${MARKER} (defect 2, part 3): report the blocker edges serialization
        // just wrote, instead of the pre-serialization snapshot.
        const createdIssueForResponse = serializationContext
            ? ((await svc.getById(issue.id)) ?? issue)
            : issue;
        res.status(201).json(createdIssueForResponse);`,
    },

    // ---------------------------------------------------------------- defect 6
    // "Assign it and start it" in one PATCH was rejected with
    // `Issue follow-up requires an assigned agent`, because the follow-up gate
    // read assigneeAgentId from the stored row and could not see the assignment
    // the same request was making. Splitting it in two worked, which is what made
    // it look like a nuisance — but it is the one write shape a recovering
    // watchdog most wants, and under the old one-write cap it was fatal.
    //
    // This widens only the branch that was previously an unconditional 409: when
    // the stored assignee is non-null, `effectiveAssigneeAgentId` is that stored
    // value and every downstream check behaves exactly as before.
    {
      file: "dist/routes/issues.js",
      find: `        if (!issue.assigneeAgentId) {
            res.status(409).json({
                error: "Issue follow-up requires an assigned agent",
                details: { issueId: issue.id, actorAgentId },
            });
            return false;
        }
        if (issue.assigneeAgentId === actorAgentId)
            return true;
        if (await hasActiveCheckoutManagementOverride(actorAgentId, issue.companyId, issue.assigneeAgentId)) {
            return true;
        }`,
      replace: `        // ${MARKER} (defect 6): judge the gate against the assignee this request
        // is setting when the row has none yet. An already-assigned issue is
        // unaffected — effectiveAssigneeAgentId is then the stored value.
        const requestedAssigneeAgentId = typeof req.body?.assigneeAgentId === "string"
            ? req.body.assigneeAgentId
            : null;
        const effectiveAssigneeAgentId = issue.assigneeAgentId ?? requestedAssigneeAgentId;
        if (!effectiveAssigneeAgentId) {
            res.status(409).json({
                error: "Issue follow-up requires an assigned agent",
                details: { issueId: issue.id, actorAgentId },
            });
            return false;
        }
        if (effectiveAssigneeAgentId === actorAgentId)
            return true;
        if (await hasActiveCheckoutManagementOverride(actorAgentId, issue.companyId, effectiveAssigneeAgentId)) {
            return true;
        }`,
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
  // `--npx-root=` names the caches to work on, it does not add to them. It used
  // to add, so `--revert --npx-root=<throwaway>` silently reverted every real
  // install as well.
  const roots =
    explicit.length > 0
      ? [...new Set(explicit)]
      : [...new Set(homes.map((home) => path.join(home, ".npm", "_npx")))];
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

/**
 * Exported function names declared more than once. Two `export function foo`
 * declarations in one module are a SyntaxError, and the server only reports it
 * at the next restart — long after the run that caused it has finished.
 */
function duplicateDeclarations(source) {
  const counts = new Map();
  for (const match of source.matchAll(/^export\s+(?:async\s+)?function\s+([A-Za-z0-9_$]+)/gm)) {
    counts.set(match[1], (counts.get(match[1]) ?? 0) + 1);
  }
  return [...counts].filter(([, count]) => count > 1).map(([name]) => name);
}

const isFailureStatus = (status) =>
  status.startsWith("anchor-") ||
  status.startsWith("duplicate-") ||
  status.startsWith("not-written") ||
  status === "missing-file";

function run(pkgRoot) {
  const version = versionOf(pkgRoot);
  if (version !== TARGET_VERSION) {
    return { pkgRoot, version, status: "skipped-version" };
  }

  const results = [];
  // Some edits anchor on text an earlier edit introduces. `apply` sees that
  // because it writes as it goes; `--check` has to simulate it in memory, or it
  // reports a phantom missing anchor for every chained edit.
  const pending = new Map();
  for (const edit of edits()) {
    const file = path.join(pkgRoot, edit.file);
    const backup = `${file}.tur73.orig`;
    if (!existsSync(file)) {
      results.push({ edit: edit.file, status: "missing-file" });
      continue;
    }
    const source = pending.get(file) ?? readFileSync(file, "utf8");

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

    if (source.includes(edit.appliedMarker ?? edit.replace)) {
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
    pending.set(file, source.replace(edit.find, edit.replace));
    results.push({ edit: edit.file, status: mode === "check" ? "would-apply" : "applied" });
  }

  if (mode === "revert") return { pkgRoot, version, status: "processed", results };

  // Nothing reaches disk until the finished file is checked. A file that is
  // already fully patched is checked too, so a corrupt install is reported even
  // when this run has nothing left to apply.
  const touched = new Set(edits().map((edit) => path.join(pkgRoot, edit.file)));
  for (const file of touched) {
    if (!existsSync(file)) continue;
    const next = pending.get(file) ?? readFileSync(file, "utf8");
    const duplicates = duplicateDeclarations(next);
    if (duplicates.length > 0) {
      const relative = path.relative(pkgRoot, file);
      for (const result of results) {
        if (result.edit === relative && result.status === "applied") {
          result.status = "not-written";
        }
      }
      results.push({
        edit: relative,
        status: `duplicate-declaration(${duplicates.join(",")})`,
      });
      continue;
    }
    if (mode === "check" || !pending.has(file)) continue;
    const backup = `${file}.tur73.orig`;
    if (!existsSync(backup)) copyFileSync(file, backup);
    writeFileSync(file, next, "utf8");
  }
  return { pkgRoot, version, status: "processed", results };
}

const installs = await findInstalls();
if (installs.length === 0) {
  console.error("No @paperclipai/server install found under ~/.npm/_npx.");
  process.exit(1);
}

let failed = false;
let duplicated = false;
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
    if (isFailureStatus(r.status)) {
      failed = true;
      if (r.status.startsWith("duplicate-")) duplicated = true;
      console.log(`  ${r.status.padEnd(20)} ${r.edit}`);
    }
  }
  for (const [status, count] of [...counts].sort()) {
    if (isFailureStatus(status)) continue;
    console.log(`  ${status.padEnd(20)} ${count}`);
  }
}

console.log(
  duplicated
    ? "\nThis install has an export declared twice, so the module will not load and\n" +
      "nothing was written. Restore it with `--revert` and re-apply, or copy the file\n" +
      "from a healthy install of the same version."
    : failed
      ? "\nSome edits did not apply. The shipped build probably changed; re-derive the anchors."
      : `\nDone (${mode}). Restart the Paperclip server for changes to take effect.`,
);
process.exit(failed ? 1 : 0);
