# meet-ai, problem statement

**Date:** 2026-09-02
**Scope:** v1, macOS, personal use.
**Related:** [`SPEC.md`](../SPEC.md) is the build contract. [`findings.md`](./findings.md) holds the evidence. This file says why any of it should exist.

---

## Who it is for

Developers who already work inside an agent, Claude Code, Codex or Cursor, and who sit in 3 to 6 calls a week where real technical decisions get made.

v1 has exactly one user, me. Public release is a v2 question (L17), so nothing here is written for a buyer.

## What goes wrong today

Four separate failures, all of them normal enough that people stopped noticing.

**Decisions evaporate.** A standup settles on Redis for sessions. Nobody writes it down. Three weeks later a PR ships an in-memory cache and the argument restarts from zero.

**Bots are not welcome in half the calls.** Otter, Fireflies and the rest join as a participant. The bot appears in the attendee list, guests see it, plenty of companies block it by policy, and Slack huddles and ad-hoc calls get no coverage at all. A tool that only works when the call is scheduled and the org allows bots covers maybe half of a real week.

**The audio leaves the machine.** Granola and every cloud notetaker upload the recording. That is a conversation about unreleased code sitting on someone else's server. For a lot of teams that ends the discussion.

**Notes stop at notes.** Even a perfect summary leaves the same manual chain: read the summary, open the tracker, retype the ticket, find the repo, paste enough context into an agent to start work. Every step is small. Together they are why most action items never move.

## Why the existing tools do not fix it

Checked live, not recalled (FINDINGS §1).

Granola records first and transcribes on stop, in the cloud, from Electron. It is a good notes product and it is the wrong shape for a privacy claim.

Muesli and Meetily both run locally and both do the hard capture work well. Both stop at a summary. Neither one hands the work to your agent.

pasrom/meeting-transcriber does call the Claude Code CLI, which proves the idea works in a shipped app. It is a transcriber with a summarizer bolted on, not a loop into the tracker and the repo.

Nobody in that list closes the last step. That gap is the whole product.

## What it should do

Written as plain requirements, in the order a day with the app actually happens.

Items 1 to 35 are the core: record with no bot, transcribe live, store the text, hand the work to my agent. Those four are what I asked for, plus what they turn out to need.

Items 36 to 73 are the rest of the same use case, pulled out of `SPEC.md` so they are stated in words and not only in tables. Nothing here is new scope; it is the same v1, written so a gap is visible before it is built.

### Recording

1. It should record a meeting without anything joining the meeting. No bot in the attendee list, no meeting link, no invite.
2. It should record both sides. My mic on one track, everything coming out of the speakers on the other.
3. It should work the same in Zoom, Meet, Teams, a Slack huddle, or a phone call on speaker, because it never touches the meeting itself.
4. It should notice a meeting probably started and ask me before it records. It should never start on its own.
5. It should ask for microphone and audio permission once, and if I say no, it should tell me what stops working and give me a button to fix it.
6. It should keep recording when I switch to AirPods mid-call.
7. It should not lose the recording if the app crashes or I force-quit it.
8. If I start it myself with no calendar event behind it, it should name the folder by date and time and let me rename the meeting afterwards.

### Transcribing

9. It should turn speech into text while I am still talking, not after the call ends.
10. It should show that text live in the window as the call runs.
11. It should mark every line as `You` or `Others`, so I can tell who said what.
12. It should write nothing when nobody is speaking. Silence must produce zero lines.
13. It should run on my machine. It should work with no internet and no API key.
14. It should transcribe English out of the box. Another language should be one line in the settings file, not a different app.
15. When it mishears a word, I should correct it in my notes, not in the transcript. The transcript is the record of what the machine heard, and every timestamp and every ticket points into it.

### Storing

16. It should store the transcript as a plain markdown file I can open in any editor.
17. It should put one folder per meeting under `~/Meetings/`, with the transcript, my notes, and the tickets inside it.
18. It should let me type notes during the call, in the same window, without stopping the recording.
19. It should let me search across every meeting I have ever recorded.
20. It should survive me deleting its database. The markdown is the real thing. The index is rebuilt.
21. It should delete the audio after 7 days and keep the text forever. I should be able to change that number.
22. It should never upload the audio or the transcript anywhere.
23. It should save my notes as I type them. No save button, and it must never move my cursor while it writes the file underneath me.
24. It should rebuild its search index by itself on the next launch. I should never have to ask for a rescan.
25. Search should cover my notes too, not only the transcripts.
26. It should delete old audio on its own, and catch up on any it missed if the Mac was off for a week.
27. It should keep a log file I can read when recording goes wrong, because that is the part most likely to break.
28. It should keep the list of people from the calendar event, so `Others` still means someone I can name a month later.

### Handing work to my agent

29. It should give me a button that copies a full prompt to the clipboard, ready to paste into Claude Code or whatever agent I use.
30. The prompt should carry everything the agent needs: the transcript, where to write the results, and the exact format to write them in.
31. In v1 it should contain no AI of its own. No model, no API key, no calls to anyone. Optional built-in AI is a v2 question, sold as a personal or enterprise subscription for people who do not want the copy-paste step. It stays optional, off by default, and the agent-prompt path keeps working with no subscription and no key.
32. It should notice when the agent writes the summary and the tickets back into the folder, and show them in the UI within a couple of seconds.
33. It should give me the same one-click prompt for a single ticket, with the transcript excerpt and the repo path already filled in, so the agent can start work.
34. It should not push tickets to Jira or Linear itself. The prompt tells the agent to do it with the agent's own connection.
35. The prompt should say exactly what to write back: a summary, the decisions we made, the action items, and the questions we left open. Fixed headings, so the app can find them and I always get the same shape.

### Knowing when to record

36. It should show me today's meetings straight from the Mac Calendar app, with no login and no setup (L13).
37. It should let me connect Google or Microsoft, or paste a calendar link, for the case where my Mac calendar is empty.
38. It should name the meeting itself from the calendar event, so I never type a title.
39. It should remind me a minute before a meeting starts.
40. It should also spot Zoom, Meet, Teams or Slack running, and plain audio activity, so ad-hoc calls with no invite still get caught.
41. It should stay quiet for solo calendar blocks. Two or more people, or it says nothing.
42. Before a call it should show me a short brief: what we said last time, and the recent commits in the repo this meeting is about.
43. It should let me start recording myself at any moment from a keyboard shortcut or the menu bar, with no meeting and no calendar entry involved.

### While the call is running

44. It should be a small quiet window that sits next to Zoom without slowing the machine down.
45. It should let me collapse the live transcript and see only my notes, then open it again (L18).
46. It should show plainly that it is recording, and for how long.
47. It should only write settled text to the transcript file. The live pane may show words that change as I speak; the file must never be rewritten.
48. It should stop with one click and finish writing every file before it says it is done.
49. It should warn me when I have no headphones, because then both tracks hear the same voices (L6).
50. It should keep going if the internet drops mid-call.

### After the call

51. It should open any past meeting and show the transcript, my notes, the summary and the tickets in one place.
52. It should let me search words across every meeting and jump to that line in the transcript.
53. It should show tickets with a status I can change: open, in progress, done, dropped. Plus who owns it and a size guess.
54. It should let me write or edit a ticket by hand, with no agent involved.
55. It should let me point a meeting at a repo folder, so every prompt it copies already has the right path in it.
56. If the agent writes the file in the wrong shape, it should flag the meeting as needing attention, not break or lose the file.
57. It should never overwrite the notes I typed when the agent writes its summary into the same folder.
58. It should record which agent analyzed the meeting and when, so I can tell a stale summary from a fresh one.
59. Once the agent has pushed a ticket, it should show me the tracker link on that ticket so I can click straight through.
60. It should keep the size guess on every ticket from day one, so in a few months I can look back and see how wrong my guesses usually are.

### First run and permissions

61. First run should be three steps and nothing else: give permission, get the speech model if one is needed, pick the meetings folder.
62. If I say no to permission, the record button should stay visibly switched off, with one line on what that breaks and a button that opens the right System Settings page.
63. On a new macOS it should use the built-in Apple speech engine with no download at all. On an older one it should download the model once, resume if the download breaks, and check it is not corrupt.
64. Switching speech engine should be one line in a settings file, not a reinstall.
65. If my Mac is older than the version it needs, it should say so on first run, not fail silently when I press record.

### Staying trustworthy

66. It should send no usage data, ever. No account, no sign-in, no phone-home.
67. It should hold no tracker tokens and no AI keys. The only secret it can ever hold is a calendar login, in the Keychain, and only if I connect one.
68. Everything it owns should be plain files under `~/Meetings/`, so I can back it up, open it in Obsidian, or delete it in Finder.
69. Deleting a meeting folder should delete the meeting. There should be no second copy anywhere.
70. It should write nothing outside `~/Meetings/`, and the prompts it hands my agent should say the same (L10).

### Settings and hooks

71. All settings should live in one commented JSON file with a schema, so my editor autocompletes it.
72. It should let me run my own script at three points: transcript ready, analysis done, meeting ended.
73. Every number I might want to change — how long audio is kept, which model, which tracker, which repo — should be in that one file, not buried in code.

Requirement 31 is the bet. Every competitor builds the AI in, then owns the retries, the output checking, the per-provider adapters and the key storage. Doing none of that keeps the app agent-agnostic and deletes most of the codebase.

When optional AI does arrive it goes behind the same folder contract: it writes the same files, in the same headings, into the same meeting folder. The app never grows a second way to store a summary, so a subscriber and a copy-paste user get the same shape and either can switch off the other at any time.

## What done looks like

Gates lifted from SPEC §5 and §6, in order.

- A 45 minute real Zoom call records to two WAV files, drift under 200ms end to end, surviving an AirPods swap mid-call and a `kill -9`.
- A 30 second silence fixture produces zero transcript lines, on both engines.
- `rm ~/Meetings/.app/index.db`, relaunch, every meeting and ticket and search comes back.
- Five meetings in a row produce tickets I use without hand-repairing them.
- I use it for two weeks instead of reaching for Notes.

The last one is the only test that matters. The first four are how I avoid finding out too late.

## What it fails at

Stated up front so I stop rather than grind (SPEC §5).

- Clock drift between the two streams still unsolved after two weeks in Phase 0. Rewrite native Swift, macOS only, drop Windows permanently.
- Tickets need hand-repair every single time in Phase 4. This is a transcriber, not a path to a PR. Reposition before writing more code.
- I stop using it for two weeks. The missing piece is the daily habit, calendar and pre-meeting brief, not more features.

## Not this problem

- Consent, retention policy and anything else a team deployment needs. Personal app for now.
- Built-in AI, subscriptions, billing, accounts and anything hosted. All v2, all optional when they land (31).
- What bots are actually good at: video, shareable playback links, a hosted transcript for people who were not there.
- Naming who spoke beyond `You` and `Others`. N-speaker diarization is post-v1.
- Echo when the user has no headphones. Detected and warned about, not fixed in code (L6).
- Windows, Linux, mobile, MCP server, semantic search. All deferred, with the seams built now so the port stays additive (SPEC §8).
- Editing the transcript by hand. It is append-only on purpose (SPEC §3.4); corrections live in the notes.
- Playing the audio back in the app, or scrubbing it against the transcript. The WAVs are on disk for 7 days; the text is the artefact.
- Syncing meetings between machines, or sharing one with a teammate. One Mac, one folder.

