#!/usr/bin/env node

// Checks the TUR-73 patch against the patched install, without needing a
// running Paperclip server. Covers the parts the patch actually adds: the
// classification signature, the run-row re-pin, and the scope fields the guard
// now reads. Run after apply.mjs.

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";

const TARGET_VERSION = "2026.916.1";

async function findInstall() {
  const explicit = process.argv[2];
  if (explicit) return explicit;
  const homes = [process.env.PAPERCLIP_GITHUB_HOST_HOME, homedir(), process.env.HOME].filter(
    Boolean,
  );
  for (const home of homes) {
    const root = path.join(home, ".npm", "_npx");
    if (!existsSync(root)) continue;
    for (const entry of await readdir(root, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const pkg = path.join(root, entry.name, "node_modules", "@paperclipai", "server");
      const manifest = path.join(pkg, "package.json");
      if (!existsSync(manifest)) continue;
      if (JSON.parse(readFileSync(manifest, "utf8")).version === TARGET_VERSION) return pkg;
    }
  }
  return null;
}

async function findInstalls() {
  const explicit = process.argv[2];
  if (explicit) return [explicit];
  const homes = [process.env.PAPERCLIP_GITHUB_HOST_HOME, homedir(), process.env.HOME].filter(
    Boolean,
  );
  const found = new Set();
  for (const home of homes) {
    const root = path.join(home, ".npm", "_npx");
    if (!existsSync(root)) continue;
    for (const entry of await readdir(root, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const pkg = path.join(root, entry.name, "node_modules", "@paperclipai", "server");
      const manifest = path.join(pkg, "package.json");
      if (!existsSync(manifest)) continue;
      if (JSON.parse(readFileSync(manifest, "utf8")).version === TARGET_VERSION) found.add(pkg);
    }
  }
  return [...found];
}

const pkgRoot = await findInstall();
if (!pkgRoot) {
  console.error(`No @paperclipai/server v${TARGET_VERSION} install found.`);
  process.exit(1);
}

const scope = await import(path.join(pkgRoot, "dist/services/task-watchdog-scope.js"));
const {
  taskWatchdogObservedSignature,
  taskWatchdogRecoveryAttributableIssueIds,
  repinTaskWatchdogSelfWriteSignature,
  resolveTaskWatchdogMutationScope,
} = scope;

const checks = [];
function check(name, fn) {
  try {
    fn();
    checks.push([true, name]);
  } catch (error) {
    checks.push([false, `${name} — ${error.message}`]);
  }
}
async function checkAsync(name, fn) {
  try {
    await fn();
    checks.push([true, name]);
  } catch (error) {
    checks.push([false, `${name} — ${error.message}`]);
  }
}

// --- signature -------------------------------------------------------------
const stopped = {
  state: "stopped",
  stopFingerprint: "task_watchdog_stop:aaa",
  includedIssueIds: ["i2", "i1"],
};

check("signature is stable regardless of id order", () => {
  assert.equal(
    taskWatchdogObservedSignature(stopped),
    taskWatchdogObservedSignature({ ...stopped, includedIssueIds: ["i1", "i2"] }),
  );
});

check("a different stop fingerprint is a different signature", () => {
  assert.notEqual(
    taskWatchdogObservedSignature(stopped),
    taskWatchdogObservedSignature({ ...stopped, stopFingerprint: "task_watchdog_stop:bbb" }),
  );
});

check("a new issue in the subtree is a different signature", () => {
  assert.notEqual(
    taskWatchdogObservedSignature(stopped),
    taskWatchdogObservedSignature({ ...stopped, includedIssueIds: ["i1", "i2", "i3"] }),
  );
});

check("two live verdicts over different issues differ", () => {
  const base = { state: "live", includedIssueIds: ["i1"], liveIssueIds: ["i1"] };
  assert.notEqual(
    taskWatchdogObservedSignature(base),
    taskWatchdogObservedSignature({ ...base, liveIssueIds: ["i2"] }),
  );
});

check("stopped and live never collide", () => {
  assert.notEqual(
    taskWatchdogObservedSignature(stopped),
    taskWatchdogObservedSignature({
      state: "live",
      includedIssueIds: ["i2", "i1"],
      liveIssueIds: ["i1"],
    }),
  );
});

check("junk classifications produce no signature", () => {
  for (const value of [null, undefined, {}, [], "stopped", { state: "" }]) {
    assert.equal(taskWatchdogObservedSignature(value), null);
  }
});

// --- recovery attribution --------------------------------------------------
// Which issues does the classifier blame for the subtree no longer being
// stopped? The guard allows a run to keep writing only when it wrote to all of
// them, so this list being right is the whole safety argument.

check("a live verdict is attributed to its live issues", () => {
  assert.deepEqual(
    taskWatchdogRecoveryAttributableIssueIds({
      state: "live",
      includedIssueIds: ["i1", "i2"],
      liveIssueIds: ["i2"],
    }),
    ["i2"],
  );
});

check("a pending-first-run verdict is attributed to its pending issues", () => {
  assert.deepEqual(
    taskWatchdogRecoveryAttributableIssueIds({
      state: "pending_first_run",
      includedIssueIds: ["i1", "i2"],
      pendingIssueIds: ["i1"],
    }),
    ["i1"],
  );
});

check("verdicts with nobody to blame stay on the strict path", () => {
  // No attributable list means the guard falls through to the 409. Allowing
  // these would turn "the tree recovered" into "any stale review may write".
  for (const classification of [
    { state: "stopped", stopFingerprint: "task_watchdog_stop:aaa", includedIssueIds: ["i1"] },
    { state: "already_reviewed", stopFingerprint: "task_watchdog_stop:aaa" },
    { state: "not_applicable", includedIssueIds: [] },
    { state: "live", includedIssueIds: ["i1"] },
    { state: "live", includedIssueIds: ["i1"], liveIssueIds: [] },
    null,
    undefined,
    [],
    "live",
  ]) {
    assert.equal(taskWatchdogRecoveryAttributableIssueIds(classification), null);
  }
});

// --- re-pin ----------------------------------------------------------------
// Drizzle's query builder is deliberately thenable — the patched code awaits it
// via `.then(rows => ...)`, so these stand-ins have to be thenable too.
function fakeDb(row) {
  const writes = [];
  return {
    writes,
    select: () => ({
      from: () => ({
        // biome-ignore lint/suspicious/noThenProperty: mimics drizzle's thenable query builder
        where: () => ({ then: (resolve) => resolve(row ? [row] : []) }),
      }),
    }),
    update: () => ({
      set: (values) => ({
        where: () => {
          writes.push(values);
          return Promise.resolve();
        },
      }),
    }),
  };
}

const watchdogScope = {
  kind: "watchdog",
  watchdogId: "w1",
  companyId: "c1",
  watchedIssueId: "src",
  watchdogIssueId: "wd",
  stopFingerprint: "task_watchdog_stop:aaa",
  runId: "run-1",
  selfWriteSignature: null,
};

await checkAsync("re-pin merges into the run's existing context snapshot", async () => {
  const db = fakeDb({
    contextSnapshot: {
      issueId: "src",
      source: "watchdog",
      taskWatchdog: { watchedIssueId: "src", stopFingerprint: "task_watchdog_stop:aaa" },
    },
  });
  await repinTaskWatchdogSelfWriteSignature(db, watchdogScope, "stopped|task_watchdog_stop:bbb");
  assert.equal(db.writes.length, 1);
  const snapshot = db.writes[0].contextSnapshot;
  assert.equal(snapshot.issueId, "src", "unrelated context keys must survive");
  assert.equal(snapshot.source, "watchdog");
  assert.equal(snapshot.taskWatchdog.watchedIssueId, "src", "watched issue id must survive");
  assert.equal(
    snapshot.taskWatchdog.stopFingerprint,
    "task_watchdog_stop:aaa",
    "original pin must survive",
  );
  assert.equal(snapshot.taskWatchdog.selfWriteSignature, "stopped|task_watchdog_stop:bbb");
});

await checkAsync("re-pin keeps a legacy taskWatchdog:true snapshot readable", async () => {
  const db = fakeDb({
    contextSnapshot: {
      taskWatchdog: true,
      watchedIssueId: "src",
      stopFingerprint: "task_watchdog_stop:aaa",
    },
  });
  await repinTaskWatchdogSelfWriteSignature(db, watchdogScope, "stopped|task_watchdog_stop:bbb");
  const snapshot = db.writes[0].contextSnapshot;
  assert.equal(snapshot.watchedIssueId, "src", "top-level fallback keys must survive");
  assert.equal(snapshot.stopFingerprint, "task_watchdog_stop:aaa");
  assert.equal(snapshot.taskWatchdog.selfWriteSignature, "stopped|task_watchdog_stop:bbb");
});

await checkAsync("re-pin is a no-op when the signature has not moved", async () => {
  const db = fakeDb({
    contextSnapshot: { taskWatchdog: { selfWriteSignature: "same" } },
  });
  await repinTaskWatchdogSelfWriteSignature(db, watchdogScope, "same");
  assert.equal(db.writes.length, 0);
});

await checkAsync("re-pin refuses a scope with no run id", async () => {
  const db = fakeDb({ contextSnapshot: {} });
  await repinTaskWatchdogSelfWriteSignature(db, { ...watchdogScope, runId: null }, "sig");
  assert.equal(db.writes.length, 0);
});

await checkAsync("re-pin refuses a non-watchdog scope", async () => {
  const db = fakeDb({ contextSnapshot: {} });
  await repinTaskWatchdogSelfWriteSignature(db, { kind: "none" }, "sig");
  assert.equal(db.writes.length, 0);
});

// --- scope carries what the guard needs ------------------------------------
await checkAsync("resolved scope carries runId and selfWriteSignature", async () => {
  const runRow = {
    id: "run-1",
    companyId: "c1",
    agentId: "a1",
    contextSnapshot: {
      taskWatchdog: {
        watchedIssueId: "src",
        stopFingerprint: "task_watchdog_stop:aaa",
        selfWriteSignature: "stopped|task_watchdog_stop:bbb",
      },
    },
  };
  const watchdogRow = {
    id: "w1",
    companyId: "c1",
    issueId: "src",
    watchdogAgentId: "a1",
    watchdogIssueId: "wd",
    status: "active",
  };
  let call = 0;
  const db = {
    select: () => ({
      from: () => ({
        // biome-ignore lint/suspicious/noThenProperty: mimics drizzle's thenable query builder
        where: () => ({ then: (resolve) => resolve([call++ === 0 ? runRow : watchdogRow]) }),
      }),
    }),
  };
  const resolved = await resolveTaskWatchdogMutationScope(db, {
    type: "agent",
    agentId: "a1",
    runId: "run-1",
    companyId: "c1",
  });
  assert.equal(resolved.kind, "watchdog");
  assert.equal(resolved.runId, "run-1", "guard cannot re-pin without the run id");
  assert.equal(resolved.selfWriteSignature, "stopped|task_watchdog_stop:bbb");
  assert.equal(resolved.stopFingerprint, "task_watchdog_stop:aaa", "original pin still surfaced");
});

// --- the route file still parses and the edits are present ------------------
const routeSource = readFileSync(path.join(pkgRoot, "dist/routes/issues.js"), "utf8");
check("route imports the new helpers", () => {
  assert.match(
    routeSource,
    /repinTaskWatchdogSelfWriteSignature, taskWatchdogObservedSignature, taskWatchdogRecoveryAttributableIssueIds, \} from "\.\.\/services\/task-watchdog-scope\.js"/,
  );
});
check("no guard site bypasses the freshness helper", () => {
  // The only direct revalidate calls left are the two inside the patch's own
  // helpers: the freshness check itself and the post-write recompute. Every
  // guard site now calls taskWatchdogMutationFreshness instead.
  const raw = routeSource.split("await taskWatchdogsSvc.revalidateMutationScope(").length - 1;
  assert.equal(raw, 2, `expected 2 direct revalidate calls, found ${raw}`);
  const guarded = routeSource.split("await taskWatchdogMutationFreshness(").length - 1;
  assert.equal(guarded, 3, `expected 3 guard sites, found ${guarded}`);
});
check("the run's own recovery is checked against the activity log, not a re-pin", () => {
  // Provenance has to come from a record written inside the mutation, not from
  // a signature captured on the previous response — the scheduler's side
  // effects land after that response finishes, so a re-pin always loses.
  assert.match(routeSource, /async function taskWatchdogRunWrittenIssueIds\(scope\)/);
  assert.match(
    routeSource,
    /eq\(activityLog\.runId, scope\.runId\), eq\(activityLog\.entityType, "issue"\)/,
  );
  assert.match(routeSource, /attributable\.every\(\(issueId\) => written\.has\(issueId\)\)/);
  // An empty write set must never satisfy `every`, which is vacuously true.
  assert.match(routeSource, /written\.size > 0 && attributable\.every\(/);
});
check("comments are no longer gated by the freshness check", () => {
  assert.match(
    routeSource,
    /\(defect 6\): comments are records, not state changes\.\n {12}return true;\n {8}\}\n {8}const boundaryDecision = await decideIssueAccess\(req, issue, "issue:comment"\);/,
  );
  // The subtree scope check must still run before that return: dropping the
  // freshness check must not let a watchdog comment outside its own subtree.
  const start = routeSource.indexOf("async function assertAgentIssueCommentAllowed(");
  assert.notEqual(start, -1, "comment guard not found");
  const body = routeSource.slice(start, routeSource.indexOf('"issue:comment"', start));
  assert.match(body, /taskWatchdogScopeAllowsIssueMutation\(db, watchdogScope, issue\)/);
  assert.match(body, /if \(scopeResult\.kind === "invalid"\)/);
  assert.doesNotMatch(body, /assertFreshTaskWatchdogSourceMutation/);
});
check("serialization anchor is filtered by follow-up author", () => {
  assert.match(
    routeSource,
    /findCurrentSerializedWatchdogChild\(parent, serializationContext\.followUpAuthorAgentId/,
  );
  assert.match(
    routeSource,
    /findCurrentSerializedWatchdogChild\(sourceIssue, serializationContext\.followUpAuthorAgentId/,
  );
  assert.match(routeSource, /eq\(issueRows\.createdByAgentId, followUpAuthorAgentId\)/);
});
check("the watchdog's own review issue cannot become the serialization anchor", () => {
  // The author filter alone still matched the watchdog review issue: it is a
  // child of the watched issue created by the same agent. That is how TUR-140
  // was born blocked behind TUR-136.
  assert.match(routeSource, /notInArray\(issueRows\.originKind, \[TASK_WATCHDOG_ORIGIN_KIND\]\)/);
  // The exclusion has to sit inside the author-filtered arm. When the parent is
  // itself the watchdog issue every child really is a follow-up, and filtering
  // there would break serialization outright.
  const start = routeSource.indexOf("const authorFilter = followUpAuthorAgentId");
  assert.notEqual(start, -1, "author filter not found");
  const arm = routeSource.slice(start, routeSource.indexOf(": [];", start));
  assert.match(arm, /notInArray\(issueRows\.originKind/);
});
check("TASK_WATCHDOG_ORIGIN_KIND and notInArray are in scope at the anchor site", () => {
  // Both are used by the edit above; a build that stopped importing either would
  // turn the filter into a ReferenceError at request time, not at load time.
  assert.match(
    routeSource,
    /\bnotInArray\b[^\n]*from "drizzle-orm"|import \{[^}]*\bnotInArray\b[^}]*\} from "drizzle-orm"/s,
  );
  assert.match(routeSource, /TASK_WATCHDOG_ORIGIN_KIND/);
});
check("the child-create 201 reports the blocker edges serialization wrote", () => {
  assert.match(routeSource, /const createdIssueForResponse = serializationContext/);
  assert.match(routeSource, /res\.status\(201\)\.json\(createdIssueForResponse\)/);
  // The re-read must come after the edges are written, or it reports the same
  // stale snapshot it replaced.
  const block = routeSource.indexOf("currentChildIssueId: currentSerializedChild?.id ?? issue.id,");
  assert.notEqual(block, -1, "serialization call site not found");
  assert.ok(
    routeSource.indexOf("const createdIssueForResponse", block) > block,
    "the response re-read runs before blockWatchdogParentOnCurrentChild",
  );
});
check("the follow-up gate sees an assignment made by the same request", () => {
  assert.match(
    routeSource,
    /const requestedAssigneeAgentId = typeof req\.body\?\.assigneeAgentId === "string"/,
  );
  assert.match(
    routeSource,
    /const effectiveAssigneeAgentId = issue\.assigneeAgentId \?\? requestedAssigneeAgentId/,
  );
  // The old unconditional read must be gone from the gate, or an already-patched
  // build could still 409 on the shape this fixes.
  assert.doesNotMatch(
    routeSource,
    /if \(!issue\.assigneeAgentId\) \{\n {12}res\.status\(409\)\.json\(\{\n {16}error: "Issue follow-up requires an assigned agent"/,
  );
  assert.match(routeSource, /if \(effectiveAssigneeAgentId === actorAgentId\)/);
});
check("the gate widens only the previously-unconditional-409 branch", () => {
  // `issue.assigneeAgentId ?? requested` means a non-null stored assignee wins,
  // so an already-assigned issue behaves exactly as it did before the patch.
  // The reverse order would let any caller claim someone else's issue.
  assert.doesNotMatch(routeSource, /requestedAssigneeAgentId \?\? issue\.assigneeAgentId/);
  assert.match(
    routeSource,
    /hasActiveCheckoutManagementOverride\(actorAgentId, issue\.companyId, effectiveAssigneeAgentId\)/,
  );
});
check("agents may name board as an unblock owner", () => {
  assert.doesNotMatch(routeSource, /\(owner === "board" \|\| "userId" in owner\)/);
  assert.match(routeSource, /Agents may not name another user as an unblock owner/);
});
check("move-to-todo readiness honours the requested blocker set", () => {
  assert.match(routeSource, /requestedBlockerIdsForMoveToTodo/);
});
check("resume-authority readiness honours the requested blocker set", () => {
  // A plain agent PATCH off `blocked` never reaches the move-to-todo branch
  // above; it goes through assertExplicitResumeIntentAllowed, which ran its
  // own readiness check against the stored set. Caught on the live server.
  assert.match(
    routeSource,
    /const requestedBlockerIds = Array\.isArray\(req\.body\?\.blockedByIssueIds\)/,
  );
  assert.doesNotMatch(
    routeSource,
    /const readiness = await svc\.getDependencyReadiness\(issue\.id\);\n {12}if \(readiness\.unresolvedBlockerCount > 0\) \{\n {16}res\.status\(409\)/,
    "the resume-authority site still reads readiness from the stored set only",
  );
});
check("every readiness read by issue.id is one of the three known sites", () => {
  // Three legitimate reads remain, and only one of them is on a path where the
  // caller can also be rewriting the blocker set:
  //   1. recovery-action staleness   — stored set is the right question
  //   2. assertExplicitResumeIntentAllowed — patched; this is its fallback arm
  //   3. POST /comments resume       — a comment cannot carry blockedByIssueIds
  // A fourth would mean the build shifted and needs re-deriving.
  const reads = routeSource.split("svc.getDependencyReadiness(issue.id)").length - 1;
  assert.equal(reads, 3, `expected 3 readiness reads by issue.id, found ${reads}`);
  // Site 2 must be the fallback arm of the ternary, not an unconditional read.
  assert.match(routeSource, /\n {16}: await svc\.getDependencyReadiness\(issue\.id\);/);
});

// --- every install, not just the one imported above ------------------------
// apply.mjs patches every install it finds, and the server can be started from
// any of them. A re-apply once left two copies of the same export in the file —
// a SyntaxError that nothing noticed until the next restart failed.
await checkAsync("no install declares the same export twice", async () => {
  const installs = await findInstalls();
  assert.ok(installs.length > 0, "no install found");
  for (const install of installs) {
    for (const relative of ["dist/services/task-watchdog-scope.js", "dist/routes/issues.js"]) {
      const file = path.join(install, relative);
      if (!existsSync(file)) continue;
      const counts = new Map();
      const source = readFileSync(file, "utf8");
      for (const m of source.matchAll(/^export\s+(?:async\s+)?function\s+([A-Za-z0-9_$]+)/gm)) {
        counts.set(m[1], (counts.get(m[1]) ?? 0) + 1);
      }
      const duplicates = [...counts].filter(([, n]) => n > 1).map(([name]) => name);
      assert.deepEqual(duplicates, [], `${file} declares ${duplicates.join(", ")} twice`);
    }
  }
});

// Twice now an edit has been reported as `anchor-missing` on an install that was
// in fact fully patched, because a later edit rewrites the text the earlier one
// inserted and the default already-applied test is "does `replace` still appear
// verbatim". A checker that fails on a healthy install is worse than no checker:
// restart.sh reads it, and a false "some edits did not apply" stops a good
// restart. Assert the end state directly — on a patched install, --check must be
// clean.
await checkAsync("apply.mjs --check calls a fully patched install clean", async () => {
  const { execFile } = await import("node:child_process");
  const { promisify } = await import("node:util");
  const applyPath = path.join(path.dirname(new URL(import.meta.url).pathname), "apply.mjs");
  const { stdout } = await promisify(execFile)(process.execPath, [applyPath, "--check"]);
  const offenders = stdout
    .split("\n")
    .filter((line) => /^\s{2}(anchor-|missing-file|not-written|duplicate-)/.test(line));
  assert.deepEqual(offenders, [], `--check reported failures on a patched install:\n${stdout}`);
  assert.match(stdout, /already-applied/);
});

let failures = 0;
for (const [ok, name] of checks) {
  if (!ok) failures += 1;
  console.log(`${ok ? "ok  " : "FAIL"} ${name}`);
}
console.log(`\n${checks.length - failures}/${checks.length} passed`);
process.exit(failures ? 1 : 0);
