# Manual checks: tur149

TUR-149: moving the meetings folder could leave meetings split between two
folders, with the app pointing at the old one.

The fix:

- A move copies every entry into the new folder (same-volume merge and
  cross-volume alike), flushes each file and folder, checks the copy (every
  entry there, as the same kind, every file the size the copy wrote), saves
  `root.json` through `meeting_format::write_atomic`, and only then deletes
  the originals (`src-tauri/src/meetings/root/move_tree.rs`). A failure before
  the pointer is saved takes out only what the move created and leaves the old
  folder as it was. A failed delete afterwards is a warning in the log.
- An empty destination on the same volume is still one `rename`, then the
  pointer; a pointer that cannot be saved renames the folder back.
- The old folder is never deleted recursively: known junk (`.DS_Store`) goes,
  then `remove_dir`; anything that appeared mid-move stays, with its folder.
- Both paths are canonicalised (`dunce`) before the nested and same-folder
  checks; a relative path, or one with `..`, is refused (`relative-folder`).
  A symlink is copied as a symlink.
- A `root.json` that will not parse is an error (`root-pointer-unreadable`,
  logged once per launch at error level), not a quiet fall back to
  `~/Meetings`. The webview keeps the user out of setup on it, and picking a
  folder in Settings, Files points the app at it (nothing moves) and reloads
  the onboarding flag from there. The path is stored as a `PathBuf`, never via
  lossy `display()`.
- `onboarding.json` is written through `write_atomic`.
- The logs folder is worked out from the current root each time it is asked
  (`logs::resolve`), falling back to the OS log folder; "Open logs folder"
  never makes a folder under an old root. Panic crash files follow a folder
  move (`logs::follow_root`); native crash notes whose folder is gone go to
  the OS log folder.

Ran headless here: `cargo test -p meet-ai --lib` for `meetings::`, `logs::`,
`onboarding::` and `folder_move` (failure injected mid-copy on a same-volume
merge and on a cross-volume copy, a short copy, the pointer failing after a
rename, a copy and a merge, originals that cannot be deleted, a meeting
arriving mid-delete, a symlink copied as a symlink, nested through a symlink,
a case variant on this case-insensitive APFS disk, relative and `..` paths,
the pointer file missing, damaged, empty and rewritten), clippy, fmt,
`pnpm vitest run src/hooks/useChangeFolder.test.ts src/ipc/errors.test.ts
src/App.test.tsx`, typecheck, biome, and the quality gate. No app was
launched and nothing touched `~/Meetings` or the real `root.json`.

## Run by hand

Use a signed build of this branch.

1. **Move to a new folder on the same disk.** Settings, Files, change the
   folder to a new empty folder. Expected: every meeting shows in the list;
   the old folder is gone.
2. **Merge into an occupied folder.** Pick a folder that already has other
   files (and a `.DS_Store`). Expected: meetings appear beside them; the old
   folder is gone; no `folder-conflict` over `.DS_Store`.
3. **Cross-volume move.** Pick a folder on a USB stick or another APFS
   volume. Expected: all meetings there, old folder gone. Then pull the stick
   out halfway through a large move (or fill it up). Expected: an error, the
   app still lists every meeting from the old folder, the stick has none of
   the half copy left.
4. **Damaged pointer.** Quit, then truncate
   `~/Library/Application Support/meet-ai/root.json` to half its text and
   launch. Expected: no setup screens; the meetings screen says meet-ai lost
   track of the folder. Settings, Files, change folder, pick the real meetings
   folder: the confirm says "keep your meetings in", and the list comes back.
   The log has one `the meetings folder pointer will not read` line.
5. **Logs after a move.** Move the folder, then Settings, Files, "Open logs
   folder". Expected: opens `<new root>/.app/logs` (or the OS log folder);
   the old meetings folder is not made again. Moving back to the old location
   is not refused over `.app`.
6. **Windows (verify it):** a meetings folder with a symlink in it, moved
   across drives without Developer Mode. Expected: the move fails and takes
   its copy back out (making a symlink needs the privilege), every meeting
   still in the old folder. A read-only file is copied but not flushed by
   meet-ai (`FlushFileBuffers` needs write access).

## Known limits

- The log plugin's file target is fixed at launch. After a cross-volume or
  merge move, lines logged later in that launch go to the original file
  (unlinked on macOS and Linux; on Windows it cannot be deleted and stays as
  a leftover in the old folder) until the app restarts.
