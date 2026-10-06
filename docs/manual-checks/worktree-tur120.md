# TUR-120: Codex sync without built-in tools and skills

Skipped here: `codex` is not installed on this worker Mac, so the two no-model
re-checks from the ticket could not be repeated (the list comes from the
ticket, checked on codex-cli 0.152.1).

## Re-check the flags (no model call)

Run: `codex features list --disable apps --disable plugins --disable shell_tool --disable image_generation --disable view_image --disable multi_agent --disable browser_use --disable computer_use`
Expected: all eight shown off; `unified_exec` still on (it is left out on purpose).

Run: `codex debug prompt-input -c skills.include_instructions=false`
Expected: no skills entries (26 before, 0 after).

## A real sync

Run a ticket sync with Codex as the agent, with a tracker MCP server set up.
Expected: the issue is created and the reply is valid; Codex does not use its shell or other built-in tools.
Why skipped: needs a signed-in Codex account and a real tracker.

## Web search

No verified flag turns Codex web search off for a sync run, so it is left out. Verify it if a flag appears.
