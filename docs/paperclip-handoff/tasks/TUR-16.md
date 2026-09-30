# TUR-16 — Agents share one git checkout and uncommitted work is being lost

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **done** |
| Priority | high |
| Owner | Alen |
| Created | 2026-09-27 08:33 UTC by Vox |
| Completed | 2026-09-28 06:09 UTC |
| Kind | Paperclip-only housekeeping (not meet-ai product work) |

## Description

Raising this because it has already destroyed work once, and it is not something I can fix from inside a code ticket.

#### What happened

While working [TUR-6](TUR-6.md) today I found `crates/modelfetch/src/lib.rs` missing from the working tree. The crate had its `Cargo.toml` and its `meet-stt-model` binary, but no library, and the whole crate was untracked — so the workspace did not compile and git held no copy. No stash, one worktree, no build artifacts. The file was simply gone.

I reconstructed it and committed it as `f60f289`. That specific hole is closed.

#### Why it is a process problem, not a one-off

Several agents appear to be writing to the same checkout at `/Users/shantanujumde/apps/meet-ai` concurrently. Observed within about ten minutes:

- Every file in `crates/stt/src/` was rewritten mid-run (mtimes all moved to 13:50:43), and `stt/lib.rs` changed shape between two reads minutes apart — two error variants present in the first read were gone in the second.
- `crates/modelfetch/Cargo.toml` briefly lost its `thiserror` dependency and regained it, which showed up as a spurious compile failure in the middle of a `cargo check`.
- `crates/audio/src/segments.rs` was transiently uncompilable — a real mid-edit state from another agent's run.
- A commit (`a42cf17`, Phase 1) landed from a run other than mine while I was working, capturing `crates/stt` but not `crates/modelfetch`, which is how the library ended up orphaned and then lost.

The combination that bites: **large amounts of work sitting untracked, in a tree multiple runs write to.** Any run that cleans, resets or re-syncs takes uncommitted work with it, and nothing recovers it.

#### What I did about my own part

Committed only my own paths and left other agents' in-flight edits untouched. I did not revert anything.

#### What needs a decision above me

I do not own workspace strategy, so I am not going to pick one. The options as I see them:

1. **Per-agent worktrees** — each agent gets its own git worktree off the same repo and merges. Strongest isolation, some merge cost.
2. **Serialise writes to the shared checkout** — only one agent holds it at a time. Simple, but it turns parallel work into a queue.
3. **Commit-early discipline plus a guard** — leave the shared checkout, but require every run to commit or stash before exiting, and fail a run that exits with untracked source. Cheapest, and it would have prevented this exact loss.

My preference is (1) if the merge cost is acceptable, with (3) as the floor regardless, because (3) is cheap and independently useful.

#### Success condition

No agent can lose another agent's uncommitted work by writing to the shared tree, and a run that exits with untracked source in `crates/` is detected rather than silently trusted.

#### Not urgent in the sense of blocking me

I am unblocked and continuing on [TUR-15](TUR-15.md). This is filed so the next loss is not a surprise.

## Commits that mention this task

- `e4a46f2` 2026-09-28 — TUR-16: per-run worktrees — document the workspace model
- `8ec71db` 2026-09-27 — TUR-16: snapshot the shared checkout so concurrent runs cannot destroy work

## Questions you answered

- **Q:** How should agent runs share the meet-ai repo from now on?
  **A:** Give each run its own worktree (recommended)
- **Q:** When a run ends with new source files that were never committed, should that stop the run or just be recorded?
  **A:** Record it and warn (current behaviour)

## Comments (4)

### Alen · 2026-09-28 06:09 UTC

> ## Done — per-run worktrees are on, and the cause was not quite what the ticket assumed
>
> You picked **worktrees** for isolation and **record-and-warn** for untracked files at exit. Both are now in place. But while wiring it I found the actual reason every run landed in the same checkout, and it was worth fixing more than the strategy flag was.
>
> ### The real cause
>
> The project was never pointed at this repo. Its code folder was `.paperclip/instances/default/projects/.../\_default` — a directory that is **empty except for a `.claude/` folder**, and that sits *inside* `/Users/shantanujumde/apps/meet-ai`.
>
> So every run started in an empty folder, found no code, and walked up the directory tree until git answered. Git answered with `/Users/shantanujumde/apps/meet-ai`. Six agents, six "separate" workspaces, one working tree — by accident, not by policy. No isolation setting would have helped while that was true, because a worktree placed under `.paperclip/` would have been walked straight past too.
>
> ### What changed
>
> 1. **The project now points at the real repo.** Registered `/Users/shantanujumde/apps/meet-ai` as its primary workspace, default ref `main`.
> 2. **Runs get a worktree.** Default mode `isolated_workspace`, strategy `git_worktree`, branched from `main`, created under `/Users/shantanujumde/apps/.meet-ai-worktrees` — deliberately **outside** the repo, so a `git clean` in one tree cannot reach the others.
> 3. **The shared-checkout lock stays as a fallback** (`serialize`), for anything that still lands in the shared tree.
>
> Verified by provisioning a worktree by hand and removing it: inside it, `git rev-parse --show-toplevel` returns the worktree, not `/Users/shantanujumde/apps/meet-ai`. That is the specific property that makes the walk-up impossible. The snapshot hooks are committed, so they travel into every worktree, and snapshot refs are shared across worktrees — a file saved in one is recoverable from any of them.
>
> ### Record-and-warn, as you chose
>
> `.claude/hooks/tree-snapshot.sh` (committed earlier as `8ec71db`) commits the whole tree — tracked changes *and* untracked, non-ignored files — to `refs/tree-snapshot/`. It runs at session start, session end, and before any command that looks destructive. A run ending with untracked source gets a named warning and a saved copy; it is **not** blocked. Seven snapshots on disk already.
>
> ```
> .claude/hooks/tree-snapshot.sh list
> git checkout <ref> -- path/to/file
> ```
>
> Documented in `CONTRIBUTING.md` (`e4a46f2`) so it is discoverable without reading this ticket.
>
> ### Leo's two asks from the incident log
>
> Both were symptoms of the shared tree, and both should stop now. The lock that refused Leo's attachments only fires on shared-checkout contention; with each run in its own worktree there is nothing to contend for, so a run with evidence can post it. Same for two live runs of one agent colliding on `design-system/` — they now collide on nothing.
>
> If duplicate runs on one issue keep happening *after* this, that is a scheduler bug rather than a workspace bug, and I will file it separately with the evidence.
>
> ### Two things for you
>
> **One to check.** Per-run worktrees may also need an instance-level toggle (Settings → Experimental, isolated workspaces). That endpoint is board-only, so I cannot read it. The proof is cheap: the next run on any issue should report a working directory under `.meet-ai-worktrees/`. If it still says `/Users/shantanujumde/apps/meet-ai`, the toggle is off — tell me and I will say exactly what to flip.
>
> **One trade-off you are now paying.** Worktrees do not carry `target/` or `node_modules/`, because both are ignored. A fresh worktree means a cold Rust build. If that gets painful I can add a provision command that symlinks or warms them.
>
> ### One hazard this does not close
>
> `git clean -xdf` at the repo root deletes `.paperclip/` — the live Paperclip database — because it is ignored, and snapshots skip ignored paths. Nothing recovers it. I have written the warning into `CONTRIBUTING.md` but have **not** added a hook that blocks the command, because you asked for warn-not-stop and this felt like your call rather than mine. Say the word and I will make that one case a hard block.

### Paperclip (automatic) · 2026-09-28 06:03 UTC

> Paperclip cannot safely continue automatic recovery because the original assignee is not invokable. The source assignment is unchanged and the board must choose the next action.

### Alen · 2026-09-27 14:03 UTC

> ## Incident log: same agent, two live runs, same files — and the one holding the lock had the wrong hypothesis
>
> Logging a concrete instance for this ticket, because it is a **new failure shape** and worse than the one in the title. This is not two different agents colliding. It is **one agent, Leo, running twice at once on the same issue and the same files.**
>
> ### What happened
>
> - Both runs were live on [TUR-22](TUR-22.md) editing `design-system/meet-ai/brand/tools/`.
> - Run `76ed7677` found it by watching `render.sh` change underneath a read it had already done.
> - Run `f1e1519a` held the checkout lock. It was editing `build.mjs` and `geometry.mjs`, producing new `meet-ai-appicon-masked-*.svg` and `-tile32-` files — the **full-bleed artwork route**.
> - Run `76ed7677` had just measured, on a real signed bundle, that the full-bleed route **does not fix the defect** — it still plates when the small reps are present, and it picks up a second bug where the bottom rows come back with the blue channel zeroed.
>
> So the run with the evidence could not write, and the run that could write was building the wrong thing.
>
> ### The part that matters for this ticket
>
> The lock did not just serialise edits — **it suppressed the evidence**. `76ed7677` tried to attach its six proof PNGs to TUR-22 and TUR-14 and the checkout lock **refused the attachments**. Its workaround was to commit them into the repo at `design-system/meet-ai/brand/proofs/leo-tur22-*.png` and post the findings on a *different* issue.
>
> That is the real cost here. The lock is meant to protect the working tree, but it also gated a **read-only, additive write to the issue thread**. The result: correct measurements ended up on the wrong ticket, and a human (me) had to go move the conclusion by hand.
>
> ### Two concrete asks for whatever fix lands here
>
> 1. **Attachments and comments must never be gated by a checkout lock.** A run that cannot edit files should still be able to say what it found. Blocking that turns a write conflict into an information loss.
> 2. **A second run of an agent that already has a live run on an issue should not start.** The current behaviour is worse than a queue — it spends a full heartbeat of work that then has to be discarded, and it did so here on a hypothesis that had already been disproved.
>
> ### Outcome this time
>
> Self-resolved, but only by luck of timing: `f1e1519a` released at 13:56, `76ed7677` had already posted its findings at 13:53. The fix (drop the 16pt/32pt reps from `icon.icns`) is now in the working tree — **uncommitted**, which is the other failure mode this ticket is named for.

### Alen · 2026-09-27 08:39 UTC

> Guard is in and verified. Now putting the actual decision in front of you.
