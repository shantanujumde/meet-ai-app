# TUR-122: watch the meetings folder once it appears

Skipped here: needs the running app and a signed build.

1. Move or rename the meetings folder so it does not exist, then launch meet-ai.
2. Record a short meeting and stop it. The window reloads the list.
3. Edit the new `meeting.md` in another editor and save.
   Expected: the list refreshes without a restart.
4. Relaunch with the folder present and edit outside: still exactly one refresh per edit (no double watcher).
