#!/usr/bin/env node

// TUR-73 checks 7 and 8, taken from real board state instead of a probe rig.
//
// Checks 7 and 8 live inside code that only runs for a task-watchdog run, so no
// ordinary API call reaches them. Three probe rigs were built to force one and
// all three failed — the third because creating throwaway issues on instruction
// is a denied operation for a watchdog, which is the correct refusal. See
// LIVE-CHECKS.md, "do not build a fourth rig".
//
// So this takes the other route: wait for a *real* watchdog follow-up and read
// the verdict off it afterwards. Run it at any time; it is read-only and it
// says plainly when there is nothing yet to measure.
//
//   node tools/paperclip-tur73/check78-scan.mjs
//
// Exit 0 = pass, or nothing to measure yet. Exit 1 = a check failed.

const RAW_BASE = process.env.PAPERCLIP_API_URL ?? "http://127.0.0.1:3100";
const BASE = RAW_BASE.replace(/\/$/, "").replace(/\/api$/, "");
const KEY = process.env.PAPERCLIP_API_KEY;
const COMPANY = process.env.PAPERCLIP_COMPANY_ID;

const WATCHDOG_REVIEW_ORIGIN = "task_watchdog";

// Only a follow-up created by a watchdog run on the *patched* server is
// evidence. Six real follow-ups predate the patch, including TUR-72 from the
// original report, and they all read clean today — TUR-72 because I unblocked
// it by hand in the very first pass. Counting those as passes would be the
// eighth false signal on this issue. Default is when the 19-edit server came
// up (2026-09-29 10:00:44 +0530); override with argv[2].
const PATCH_EPOCH = new Date(process.argv[2] ?? "2026-09-29T04:30:00Z");
// A blocker edge onto a sibling in one of these states is the defect-2 shape.
// `backlog` siblings never chained, which is why TUR-133 came back clean and
// check 3 was wrongly recorded as passing off it.
const CHAINING_STATES = new Set(["blocked", "in_review", "in_progress", "todo"]);

if (!KEY || !COMPANY) {
  console.error("PAPERCLIP_API_KEY and PAPERCLIP_COMPANY_ID must be set.");
  process.exit(2);
}

async function api(path) {
  const res = await fetch(`${BASE}${path}`, {
    headers: { Authorization: `Bearer ${KEY}` },
  });
  if (!res.ok) throw new Error(`GET ${path} -> ${res.status} ${await res.text()}`);
  return res.json();
}

const ref = (i) => (i ? `${i.identifier} (${i.status})` : "unknown issue");

async function main() {
  const issues = await api(`/api/companies/${COMPANY}/issues?limit=1000`);
  const byId = new Map(issues.map((i) => [i.id, i]));

  const reviewIssues = issues.filter((i) => i.originKind === WATCHDOG_REVIEW_ORIGIN);
  console.log(`watchdog review issues on the board: ${reviewIssues.length}`);

  // Every run that belonged to a watchdog review issue. An issue whose
  // originRunId is in this set was created *by a watchdog run*, which is the
  // only kind of run that reaches the serialization code being measured.
  const watchdogRuns = new Map(); // runId -> review issue
  for (const review of reviewIssues) {
    let runs;
    try {
      runs = await api(`/api/issues/${review.id}/runs?limit=100`);
    } catch (err) {
      console.log(`  ! could not read runs for ${review.identifier}: ${err.message}`);
      continue;
    }
    // The runs endpoint names this `runId`, not `id`. Reading the wrong field
    // gives every run the key `undefined`, the map collapses to one entry and
    // the scan reports "nothing to measure" on a board full of runs — a check
    // that fails in the direction of a clean bill of health. Refuse to guess.
    for (const run of Array.isArray(runs) ? runs : (runs.runs ?? [])) {
      const id = run.runId ?? run.id;
      if (!id) throw new Error(`run row on ${review.identifier} has no runId: ${Object.keys(run)}`);
      watchdogRuns.set(id, review);
    }
  }
  console.log(`watchdog runs found: ${watchdogRuns.size}`);
  if (reviewIssues.length > 0 && watchdogRuns.size === 0) {
    throw new Error("review issues exist but no runs were read — the runs endpoint shape changed");
  }

  // A follow-up is anything a watchdog run created that is not a review issue.
  const followUps = [];
  for (const issue of issues) {
    if (issue.originKind === WATCHDOG_REVIEW_ORIGIN) continue;
    const review = issue.originRunId ? watchdogRuns.get(issue.originRunId) : undefined;
    if (review) followUps.push({ issue, review, matchedBy: "originRunId" });
  }

  const prePatch = followUps.filter((f) => new Date(f.issue.createdAt) < PATCH_EPOCH);
  const measurable = followUps.filter((f) => new Date(f.issue.createdAt) >= PATCH_EPOCH);

  if (prePatch.length > 0) {
    console.log(
      `\n${prePatch.length} follow-up(s) predate the patch and are NOT evidence: ` +
        prePatch.map((f) => f.issue.identifier).join(", "),
    );
  }

  if (measurable.length === 0) {
    console.log("\nNothing to measure yet: no watchdog run has created a follow-up");
    console.log(`since the patch went live (${PATCH_EPOCH.toISOString()}).`);
    console.log("Checks 7 and 8 stay offline-only until one does.");
    if (reviewIssues.filter((r) => r.status !== "done").length === 0) {
      console.log("Note: no watchdog review issue is currently open.");
    }
    return 0;
  }

  let failed = 0;
  for (const { issue, review, matchedBy } of measurable) {
    const detail = await api(`/api/issues/${issue.id}`);
    const blockedBy = detail.blockedBy ?? [];
    const parent = byId.get(issue.parentId);

    console.log(`\n--- follow-up ${ref(issue)}  [matched by ${matchedBy}]`);
    console.log(`    created by watchdog run ${issue.originRunId} of ${review.identifier}`);
    console.log(`    parent ${parent ? parent.identifier : issue.parentId ?? "none"}`);
    console.log(`    blockedBy: ${blockedBy.map((b) => b.identifier).join(", ") || "(none)"}`);

    // `blockedBy` is current state, not birth state, and check 7 is about
    // birth. If anyone edited the blockers after creation — including the
    // recovery a watchdog run would do next — a clean read today says nothing.
    // Serialization writes its edges as part of the create, so rows within a
    // few seconds of createdAt are birth, not an edit.
    const born = new Date(issue.createdAt).getTime();
    const activity = await api(`/api/issues/${issue.id}/activity?limit=200`);
    const laterEdits = (Array.isArray(activity) ? activity : []).filter(
      (r) => r.action === "issue.blockers_updated" && new Date(r.createdAt).getTime() > born + 10_000,
    );
    if (laterEdits.length > 0) {
      console.log(
        `    CHECK 7 INCONCLUSIVE: blockers edited ${laterEdits.length}x after creation ` +
          `(first ${laterEdits.at(-1).createdAt}). Read the activity log for birth state.`,
      );
      console.log("    CHECK 8: not takeable here — needs the 201 body the creating run saw.");
      continue;
    }

    // Check 7 — the child must not be born blocked behind the watchdog's own
    // review issue. The review issue is a child of the watched issue and is
    // created by the same agent, which is how it slipped past the first fix.
    if (blockedBy.some((b) => b.id === review.id)) {
      console.log(`    CHECK 7 FAIL: born blocked behind its own review issue ${review.identifier}`);
      failed += 1;
    } else {
      const sibling = blockedBy.find((b) => {
        const s = byId.get(b.id);
        return s && s.parentId === issue.parentId && CHAINING_STATES.has(s.status);
      });
      if (sibling) {
        console.log(`    CHECK 7 FAIL: born blocked behind sibling ${sibling.identifier}`);
        failed += 1;
      } else {
        console.log("    CHECK 7 PASS: not born blocked behind a sibling or the review issue");
      }
    }

    // Check 8 cannot be reconstructed after the fact: it is about whether the
    // 201 *response body* showed the edges serialization wrote, and only the
    // creating run ever saw that body. A later GET shows the true edges either
    // way, which is exactly the discrepancy the check is about.
    console.log("    CHECK 8: not takeable here — needs the 201 body the creating run saw.");
    console.log(`             Look for it in the run's report on ${review.identifier}.`);
  }

  console.log(
    `\n${measurable.length} post-patch follow-up(s) examined, ${failed} check-7 failure(s).`,
  );
  return failed === 0 ? 0 : 1;
}

main().then(
  (code) => process.exit(code),
  (err) => {
    console.error(err.message);
    process.exit(2);
  },
);
