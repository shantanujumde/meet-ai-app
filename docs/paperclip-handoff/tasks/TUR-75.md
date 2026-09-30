# TUR-75 — Shared checkout: serialized runs now; revisit per-issue worktrees if it costs time

[← Back to the handoff](../HANDOFF.md)

| | |
|---|---|
| Status | **backlog** |
| Priority | low |
| Owner | Alen |
| Created | 2026-09-28 04:49 UTC by Alen |
| Kind | Paperclip-only housekeeping (not meet-ai product work) |

## Description

#### Why this exists

Every agent in this company runs against one working tree. The project workspace strategy is `project_primary`, and each run's directory (`.../projects/.../_default`) is a subdirectory of the same checkout at `/Users/shantanujumde/apps/meet-ai`. `git worktree list` shows a single entry. Concurrent runs therefore edit the same files.

This has already caused real loss. An entire untracked crate — `crates/modelfetch`'s library — disappeared while another run was rewriting neighbouring crates, and had to be reconstructed from scratch against the contract its own binary depended on (restored as `f60f289`, verified still on `main`). A second run reported `crates/stt` and `crates/audio` changing shape between two reads minutes apart.

#### What was done (2026-09-28)

Project execution workspace policy set to serialize runs on the shared checkout:

```
executionWorkspacePolicy: {
  enabled: true,
  defaultMode: "shared_workspace",
  sharedWorkspaceConcurrency: "serialize",
  allowIssueOverride: true
}
```

One writer at a time. This is the whole company — there is only one project.

#### The option deliberately not taken

`workspaceStrategy: { type: "git_worktree" }` with `defaultMode: "isolated_workspace"` gives each issue its own worktree and branch. Stronger isolation, but three costs that are not free here:

- This is a Rust workspace. Each worktree carries its own `target/`, so every new one pays a multi-GB cold rebuild.
- Merging branches back becomes standing work that currently has no owner.
- It may depend on instance experimental flags (`enableIsolatedWorkspaces`, `enableWorktreeRunExecution`) that need board access to read.

#### Trigger to revisit

Pick this up if serialization starts visibly costing time — heartbeats queueing behind long Rust builds, or agents waiting on each other to make progress. Until then serialize is the correct trade: slower is recoverable, a lost crate is not.

#### Unresolved question for the board

If we do move to per-issue worktrees, who owns merging those branches back into `main`?
