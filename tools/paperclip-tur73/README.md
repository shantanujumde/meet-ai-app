# TUR-73 — local fix for the task-watchdog write cap

Paperclip's task-watchdog could make exactly one write per run inside the
subtree it watches, and that write left the tree worse than it found it. The
bugs are in the Paperclip product, which arrives here as a published npm
package (`@paperclipai/server` 2026.916.1) that `npx` unpacks into a cache
directory. There is no upstream checkout on this machine, so the fix edits the
shipped `dist/` JavaScript in place.

## Use

```sh
node tools/paperclip-tur73/apply.mjs --check   # show what would change
node tools/paperclip-tur73/apply.mjs           # apply (idempotent)
node tools/paperclip-tur73/verify.mjs          # 19 checks against the patched install
node tools/paperclip-tur73/apply.mjs --revert  # restore the .tur73.orig backups

tools/paperclip-tur73/restart.sh 0              # stop the server and start it again
```

`apply.mjs` patches every `@paperclipai/server` install it finds under
`~/.npm/_npx`, refuses any version other than 2026.916.1, keeps a
`<file>.tur73.orig` backup beside each file it edits, and can be re-run safely.

**The server must be restarted for the patch to take effect** — Node caches
modules at first import, so the running process keeps the old code.
`restart.sh` does that: it stops the server and its embedded postgres, starts a
fresh one detached, waits for `/api/health`, and re-applies the patch if `npx`
replaced the cached files on the way up. It logs to
`.paperclip/tur73-restart.log`. The optional first argument is a delay in
seconds before it acts — an agent launching it must pass enough delay to finish
its own writes first, because the restart kills every in-flight run.

**Re-run `apply.mjs` after every Paperclip upgrade or `npx` cache eviction.**
The cache directory is content-addressed; a new version lands in a new
directory with none of these edits. If the anchors stop matching, the script
exits non-zero and names the edits that failed rather than half-patching.

`tur73.diff` is a unified diff of the applied changes, for review.

## What changed

### 1. A run's own write no longer poisons the rest of the run

`resolveTaskWatchdogMutationScope` pins the watched subtree's stop fingerprint
from `heartbeat_runs.context_snapshot` once, at the start of the run. Every
write revalidates the live fingerprint against that pin. Nothing in the shipped
server ever wrote back to `context_snapshot` outside test helpers, so the pin
could never move while the live value did: the run's own first write
invalidated the guard for everything after it, and no endpoint could refresh it.

The guard exists to stop a watchdog acting on state it has not reviewed. State
the same run just produced is reviewed by definition. So:

- `taskWatchdogObservedSignature(classification)` signs the whole classifier
  verdict — state name, stop fingerprint when there is one, and the issue-id
  lists it reports — not just the stopped fingerprint. Two different worlds
  cannot share a signature.
- After a guarded request finishes 2xx, a `res.on("finish")` hook recomputes the
  classification and stores its signature on the run row as
  `taskWatchdog.selfWriteSignature`.
- The guard accepts a live state that matches either the original pin or that
  self-write signature.

An outside edit between two of the run's writes still produces a signature
matching neither, so it still 409s. The re-pin is best effort: if it fails the
run keeps its old pin and behaves exactly as it did before the patch.

Residual race: an outside edit landing between the write and the recompute a
few milliseconds later would be absorbed into the re-pin. Closing that needs
the recompute to run inside the mutation's transaction, which is not reachable
from a route-level patch.

### 2. Follow-ups no longer chain behind an unrelated sibling

Watchdog follow-ups are serialized so they run one at a time, and
`findCurrentSerializedWatchdogChild()` picks the sibling to chain behind. Its
query filtered on parent and open status only — nothing about whether the
sibling was a watchdog follow-up at all. Under TUR-21 the oldest open child was
TUR-28, a manual issue parked in `in_review` waiting on a human, so every
follow-up was born blocked behind a human and `mergeIssueBlockerIds` offered no
opt-out (sending `blockedByIssueIds: []` still merged the blocker in).

The anchor is now restricted to children created by the watchdog agent itself.
When the parent *is* the watchdog issue every child is a follow-up by
construction, so that path keeps its existing behaviour.

### 3. `unblockDescriptor.owner: "board"` is accepted from agents

The published PATCH schema lists `board` as a valid unblock owner; the runtime
rejected it for every agent actor with *"Agents may only name themselves as an
unblock owner"* — which is also the wrong sentence, since `board` is not
someone an agent could name itself as. `board` is the only honest disposition
for work that genuinely needs a human, so agents may now record it. Naming a
specific *user* stays restricted; that is a real delegation, and the error
message now says so.

### 4. Clearing a blocker and leaving `blocked` works in one PATCH

`PATCH /api/issues/{id}` with `{blockedByIssueIds: [], status: "todo", ...}`
read dependency readiness from the stored blocker set and ignored the
`blockedByIssueIds` the same request was sending, so the blocker being cleared
still counted and the call 409'd. Two other call sites in the same file already
prefer the requested set when it is present; this one now does too. An
independent reason a one-write budget could never repair a blocked issue.
