# Manual checks: a-tur2 (TUR-2, notes schema and wrap-up.md)

`crates/prompts`: the notes JSON schema, its Rust types and check, and the
default `wrap-up.md` template with its two renders (agent and clipboard).

## Run by hand

1. **Claude Code accepts the schema and the prompt.** In an empty temp folder,
   with the rendered agent prompt (the body of
   `crates/prompts/src/snapshots/prompts__wrap_up__tests__agent.snap`, below
   the second `---`) in `prompt.md` and `NOTES_SCHEMA` from
   `crates/prompts/src/notes.rs` in `schema.json`:

   ```sh
   claude -p --output-format json --json-schema "$(cat schema.json)" --model haiku \
     --tools "" --strict-mcp-config --permission-mode dontAsk \
     --settings '{"disableAllHooks":true}' < prompt.md
   ```

   Expect: `structured_output` passes `prompts::notes::validate`; one task
   "Ship the search box", owner "Priya", due "Friday", `transcript_ref`
   `"00:00:05"` (no brackets); the Postgres choice under `decisions`.
   Why skipped: the one probe this run tried (Claude Code 2.1.286, haiku) was
   refused by the session's auto-mode permission check ("Create Unsafe
   Agents") before it started. Nothing ran and nothing was sent.
2. **Codex accepts the schema.** Same files:
   `codex exec --ephemeral --skip-git-repo-check -s read-only --model <m> --output-schema schema.json --output-last-message out.json - < prompt.md`.
   Expect: Codex starts (no "invalid schema" error from strict mode), and
   `out.json` passes `validate`.
   Why skipped: Codex is not installed on this Mac.
3. **Clipboard prompt writes the right files.** Paste the body of
   `prompts__wrap_up__tests__clipboard.snap` into an agent with a copy of a
   sample meeting folder at the path it names.
   Expect: `meeting.md` keeps its frontmatter, gains `analyzed_by: clipboard`,
   and has the four headings; `tickets/TICK-0007.md` matches SPEC §3.3.
   Why skipped: needs a signed-in agent with file access.

## Choices made without a ruling

- Schema check is `jsonschema` 0.58.4 with `default-features = false` (the
  defaults pull `reqwest` for remote `$ref`s; this crate must have no network
  client). Added to `SETUP.md` §2.4. Draft fixed to 2020-12.
- `transcript_ref` pattern is the ticket's `^\d{2}:\d{2}:\d{2}$`. `jsonschema`
  reads `\d` as `[0-9]` (ECMA rules), so non-ASCII digits are rejected; a test
  covers it.
- "The A11 example" in the tests is A11's shape filled with real values; the
  example as printed in SPEC (`"HH:MM:SS"` as the value) would fail the pattern.
- Problem lines from the check are cut at 300 characters, so an error never
  copies a large slice of the meeting into a log.
- The meeting title is wrapped as data too (`<title>`), since a calendar title
  can come from anyone. Closing `</title`, `</transcript`, `</notes` inside the
  data become `<\/…` in Rust before rendering, so user templates get that too.
- Rendering is strict: an unknown variable in a user template is
  `Error::Template`, not blank text. A user file that cannot be read for any
  reason other than "missing" is `Error::Io`, so an ignored edit is reported.
- Clipboard target: the app passes `first_ticket_id` (`TICK-0007`) already
  formatted; a due date goes in the ticket body ("Due: Friday") because §3.3 has
  no `due` field; only `analyzed_by: clipboard` is set in frontmatter.
- `tickets` folder name is a private const in `wrap_up.rs`. `store` has the
  same name, but `prompts` does not depend on `store` and `meeting_format::layout`
  has no tickets constant yet; it probably belongs there.
