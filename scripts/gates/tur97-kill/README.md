# TUR-97 kill gate

A scripted check that a recording survives the app dying in the middle of a
meeting. It starts a real recording in the real app, ends it the hard way,
relaunches the app, and then checks the files a user would open.

```
scripts/gates/tur97-kill/gate.sh [--app PATH] [--seconds N] [--end kill9|quit]
                              [--out DIR] [--keep] [--stt PATH] [--real-root]
scripts/gates/tur97-kill/gate.sh --verify-only <meeting dir> [--expect-seconds N]
```

| Option | Meaning |
| --- | --- |
| `--app PATH` | App bundle to test. Default `/Applications/meet-ai.app`. A dev build is at `target/release/bundle/macos/meet-ai.app`. |
| `--seconds N` | How long to record before ending it. Default 60. |
| `--end kill9` | `kill -9` the app while it records (the default). The app gets no chance to run any code. |
| `--end quit` | Send a normal Quit while it records, as if chosen from the menu. This shows what the app does on an ordinary quit mid-recording. |
| `--out DIR` | Where the evidence goes. Default `target/tur97-kill-gate/<time>-<end>-<N>s/`. `target/` is gitignored. |
| `--keep` | Keep the test recording even when every check passes. A failed run always keeps it. |
| `--stt PATH` | The `meet-stt` binary. Default `target/meet-stt`, built with `just sidecar`. |
| `--real-root` | Record into the app's real meetings folder, not a temp one. The test folder is moved into `--out` at the end. |
| `--verify-only DIR` | Don't record. Relaunch the app, then check an existing meeting folder. This is for the manual endings below. |
| `--expect-seconds N` | With `--verify-only`: how long that recording ran. Without it, check 3 is skipped. |

The script exits 0 only when every check passes.

## Before you run it

- Quit meet-ai. The gate refuses to start while any `meet-ai` process is
  running, because it has to know which process to kill.
- The app needs microphone and system-audio permission. The terminal needs
  Accessibility permission, because the recording starts from the global
  shortcut ⌘⇧R, which the script sends through System Events. If the
  shortcut does nothing, check 1 fails with "no new *-meeting folder".
- Turn the speakers on. The script runs `say` so that `system.wav` contains
  a known sentence.
- `drift-check` is built from this checkout (`cargo build -p audio --bin
  drift-check`), so check 6 uses the branch's own code.

## Where the recording goes

`open` on macOS 27 has an `--env NAME=VALUE` option (see `man open`). The gate
launches the app with
`open --env MEET_AI_MEETINGS_ROOT=<out>/meetings-root -a <app>`, so by default
the app records into a temp folder inside the evidence folder and the gate
never writes into `~/Meetings`. The script checks where the recording actually
landed and writes `open --env: worked` or `did NOT reach the app` in the
summary. If a recording lands in the real meetings folder anyway, the script
moves that one folder into the evidence folder. It never touches any other
folder in `~/Meetings`.

The onboarding flag is stored inside the meetings root
(`<root>/.app/onboarding.json`). A fresh temp root would open the app on its
setup screen, so the gate writes a "setup done" flag into the temp root, and
only there.

## What it does

1. Launches the app and waits 4 s. Sends ⌘⇧R, then waits for a new
   `*-meeting` folder to appear. The clock starts when it appears.
2. Plays the sentence with `say -r 170` right away, then every 2 minutes.
3. After `--seconds`, ends the app: `kill -9`, or Quit.
4. Saves what the ending left on disk as `after-end-*`: header dumps,
   `segments.json`, a directory listing. This is recorded before the relaunch
   can repair anything.
5. Relaunches the app, lets it settle for 8 s, and quits it normally.
6. Runs checks 1-7 on the meeting folder as the relaunch left it. A fix
   that repairs files when the app starts counts, the same as a user
   relaunching and then opening the meeting.
7. If every check passed and `--keep` wasn't given, deletes the test
   recording. The evidence stays either way.

## The checks and what each one proves

| # | Check | Passes when | What it proves |
| --- | --- | --- | --- |
| 0 | Quit while recording exits (`--end quit` only) | The process is gone within 20 s of Quit | A normal quit mid-recording doesn't hang. |
| 1 | meeting folder | A new folder with `audio/` appeared | The recording actually started, so the rest isn't testing an empty run. |
| 2 | WAV headers parse + consistent | Both WAVs are RIFF/WAVE, PCM 16 kHz mono 16-bit. The RIFF size equals `data_offset + data_size - 8`. The declared data size is not more than the bytes on disk. | A player can open each file, and the header doesn't promise audio that isn't there. |
| 3 | declared duration | Each header declares at least (seconds recorded − 6 s) | Playback runs for about as long as we recorded, at most one 5 s checkpoint short. This is the check that catches a header that is never updated, or one that breaks past a size limit. |
| 4 | PCM beyond header < 6 s | Less than 6 s of audio sits on disk past the declared data size | Audio that was captured isn't hidden from players. On v0.3.0 the whole recording is hidden this way. |
| 5 | segments.json | It exists, parses, and has at least 1 segment | The timing record that drift measurement and the transcript need survived. |
| 6 | drift-check | Exit 0, and no "invariant violation" line | Drift between the two tracks can still be measured after the ending, and `segments.json` doesn't claim fewer frames than a WAV header. |
| 7 | system.wav transcribes | `meet-stt system.wav` returns at least half (and at least 3) of the sentence's content words | The system track holds the real speech, not silence or noise, and reads back through its header. meet-stt runs with a time limit because it can hang on a WAV that declares 0 bytes. |
| 8 | relaunch | The app is up within 10 s, still running 8 s later, quits within 20 s, and the app log has no new `[ERROR]` or panic lines since the relaunch | The ending didn't leave anything behind that stops the app from starting or makes it fail at startup. |

### Which drift-check results are acceptable after a kill

- Exit 0 passes.
- Exit 2 ("not measurable") passes only if the recording was shorter than
  10 s and the reason is "no checkpoint anchors". A recording that short may
  not have reached its first 5 s checkpoint, so there really is nothing to
  measure from yet.
- Any other exit 2 fails. It means the ending lost the checkpoint anchors
  that drift is measured from, or it lost `segments.json`.
- Exit 1 fails. The recording drifted over the 200 ms gate.

## The 1-hour variant

The ticket requires at least one run after a recording of an hour or more.
A WAV header that only breaks past a size limit is exactly the failure this
catches.

```
scripts/gates/tur97-kill/gate.sh --seconds 3600 --end kill9
```

This needs about 62 minutes, plus the time to transcribe an hour of audio.
Keep the Mac awake and plugged in (`caffeinate -dims` in another terminal),
and don't press ⌘⇧R during the run. The script prints progress once a
minute. Each WAV grows to about 115 MB. Without `--keep`, a passing run
deletes them.

## Manual endings: lid close, logout, sleep

A script can't close the lid or log you out. For these, record by hand, end
the recording the manual way, and then let the script check the result.

For every one of these:

1. Quit meet-ai. Launch it normally (Finder or `open -a meet-ai`).
2. In a terminal, play known speech in a loop so `system.wav` has content
   check 7 can find:
   `while :; do say -r 170 "The quick brown fox jumps over the lazy dog while the recorder keeps running."; sleep 20; done`
3. Press ⌘⇧R and note the time. Let it record for at least a minute.
4. Do the ending (see below).
5. Stop the `say` loop. Make sure meet-ai is not running (quit it if it is),
   then run:
   `scripts/gates/tur97-kill/gate.sh --verify-only ~/Meetings/<the new folder> --expect-seconds <seconds of audio you expect>`

   The script relaunches the app (check 8), quits it, and runs checks 1-7.
   It never moves or deletes the folder.

| Ending | What to do | What `--expect-seconds` should be |
| --- | --- | --- |
| Lid close | Close the lid while recording. Wait at least 1 minute, then open it and log in. Stop the recording with ⌘⇧R if it's still going, then quit the app. | The seconds you recorded before closing the lid. Capture pauses while the Mac sleeps, so don't count the time the lid was shut. If the app kept recording after wake, add the seconds recorded after wake. |
| Sleep | Choose Apple menu → Sleep while recording (or run `pmset sleepnow`). Wait at least 1 minute, wake the Mac, then continue as for lid close. | Same as lid close. |
| Logout | Choose Apple menu → Log Out while recording and confirm. Log back in. The app is gone. Don't relaunch it yourself; the script does that. | The seconds from ⌘⇧R to confirming Log Out. |

Use `drift-check.out` as well. For sleep and lid close, drift-check prints a
`boundary (...)` line with the time spent asleep. If that line is missing,
the app didn't notice the sleep.

## Evidence in the out folder

| File | Contents |
| --- | --- |
| `summary.txt` | Every check line, `N/M passed`, and whether `open --env` worked |
| `run.log` | A timestamped log of what the script did |
| `after-end-{mic,system}-wav.json`, `after-end-*-header.hex`, `after-end-segments.json`, `after-end-ls.txt` | The state the ending left, before the relaunch |
| `final-{mic,system}-wav.json`, `final-*-header.hex`, `final-*-afinfo.txt`, `final-segments.json` | The state the checks ran on. `afinfo` shows the duration macOS thinks the file has. |
| `after-relaunch-ls.txt` | The folder after the relaunch. If it differs from `after-end-ls.txt`, the relaunch touched the files. |
| `drift-check.out`, `drift-check.err` | drift-check's output |
| `stt-system.jsonl`, `stt-system.err`, `stt-system-check.json` | meet-stt's output, and which sentence words it found |
| `recording-app-log.txt`, `relaunch-app-log.txt`, `relaunch-logscan.json` | App log lines from the recording and from the relaunch |
| `app-stdout.log`, `app-stderr.log`, `relaunch-app-*.log` | The app's own stdout and stderr, captured through `open --stdout/--stderr` |
| `meetings-root/` | The test recording, when the run failed or `--keep` was given |

`check.py` does the file parsing. It walks the WAV chunks without assuming a
44-byte header. Each subcommand prints one line of JSON and can be run on
its own:
`python3 scripts/gates/tur97-kill/check.py wav <file.wav> --expect-seconds 60`.

## Results (2026-09-30)

| Build | Ending | Result | Header lag | `segments.json` | drift-check |
| --- | --- | --- | --- | --- | --- |
| v0.3.0 | `kill -9` at 20 s (`--verify-only`) | **3/8** | 16.3 s (header declares 0 s) | missing | not measurable |
| TUR-97, before the quit fix | `kill -9` at 60 s | **8/8** | 3.5 s | 11 anchors | PASS, 82.8 ms |
| TUR-97, before the quit fix | Quit at 30 s | **9/9** | 5.0 s | 5 anchors | PASS, 82.7 ms |
| TUR-97 (2f70024) | Quit at 30 s | **9/9** | 0.0 s (clean stop) | 6 anchors | PASS, 18.7 ms |
| TUR-97 (2f70024) | `kill -9` at 60 s | checks 1–6 pass | 3.8 s | 11 anchors | PASS, 40.0 ms |

The last row's check 7 failed twice because other audio was playing on the Mac
during the run, so the sentence came back mixed with other speech; the files
themselves passed every other check. Launch-time repair of a v0.3.0 kill was
checked separately on a copy: headers declaring 0 s became 16.25 s
(`afinfo` agrees), and a second launch changed nothing.

Not run yet: the one-hour variant, and the manual lid close / sleep / logout
endings.
