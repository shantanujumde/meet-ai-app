#!/usr/bin/env bash
# Answer the TCC consent prompt without a human at the keyboard.
#
#   ./auto-click.sh allow "meet-ai" 90
#   ./auto-click.sh deny  "meet-ai" 90
#
# Run it in the background *before* the thing that triggers the prompt. Exits 0
# the moment it clicks and prints CLICKED:<process>:<button>; exits non-zero on
# timeout.
#
# Requires Accessibility for the process that runs it:
#   osascript -e 'tell application "System Events" to return count of every process'
# A -1743 error there means Accessibility is not granted and this cannot work.
#
# SAFETY — this is the important part. A clicker that hunts the whole desktop
# for a button named "Allow" will eventually click one in the user's browser or
# Slack. So a window is only ever touched when ALL of these hold:
#
#   1. Some static text in it contains NEEDLE (default "meet-ai"), and
#   2. it has both an affirmative and a negative button — the shape of a
#      consent dialog, not a document window that happens to mention us, and
#   3. the process owning it is on ALLOWED_PROCS below.
#
# Button matching is by apostrophe, not by the word: the real dialog reads
# "Don’t Allow" with U+2019, so a naive `contains "Allow"` matches the deny
# button too. Both curly and straight forms are checked, since the string
# differs by localisation and macOS version.
set -euo pipefail

MODE="${1:?usage: auto-click.sh <allow|deny> [needle] [timeout-seconds]}"
NEEDLE="${2:-meet-ai}"
TIMEOUT="${3:-90}"

case "$MODE" in
  allow|deny) ;;
  *) echo "mode must be 'allow' or 'deny', got '$MODE'" >&2; exit 64 ;;
esac

# Processes macOS has been observed to draw the TCC consent sheet from. Keeping
# this closed rather than open is what stops a stray click in a user app.
ALLOWED_PROCS="${AUTO_CLICK_PROCS:-UserNotificationCenter,tccd,ControlCenter,coreaudiod,SecurityAgent,universalAccessAuthWarn,UserNotificationCenterUIKit,Notification Center,NotificationCenter}"

/usr/bin/osascript - "$MODE" "$NEEDLE" "$TIMEOUT" "$ALLOWED_PROCS" <<'APPLESCRIPT'
on splitText(t, delim)
  set oldTID to AppleScript's text item delimiters
  set AppleScript's text item delimiters to delim
  set parts to text items of t
  set AppleScript's text item delimiters to oldTID
  return parts
end splitText

on run argv
  set mode to item 1 of argv
  set needle to item 2 of argv
  set deadline to (item 3 of argv) as integer
  set allowed to splitText(item 4 of argv, ",")

  repeat with _tick from 1 to deadline
    tell application "System Events"
      repeat with p in (every process)
        try
          if (count of windows of p) > 0 then
            set pname to name of p
            if allowed contains pname then
              repeat with w in (every window of p)
                -- (1) does this window actually talk about us?
                set mentionsUs to false
                try
                  repeat with s in (every static text of w)
                    set sv to value of s
                    if sv is not missing value and sv contains needle then
                      set mentionsUs to true
                    end if
                  end repeat
                end try

                if mentionsUs then
                  -- (2) consent shape: one affirmative, one negative button
                  set affirmative to missing value
                  set negative to missing value
                  repeat with b in (every button of w)
                    set bn to name of b
                    if bn is not missing value then
                      if (bn contains "’t") or (bn contains "'t") then
                        set negative to b
                      else if bn contains "Allow" or bn contains "OK" then
                        set affirmative to b
                      end if
                    end if
                  end repeat

                  if affirmative is not missing value and negative is not missing value then
                    if mode is "deny" then
                      set target to negative
                    else
                      set target to affirmative
                    end if
                    set chosen to name of target
                    click target
                    return "CLICKED:" & pname & ":" & chosen
                  end if
                end if
              end repeat
            end if
          end if
        end try
      end repeat
    end tell
    delay 1
  end repeat
  error "TIMEOUT: no consent dialog mentioning " & needle & " after " & deadline & "s" number 2
end run
APPLESCRIPT
