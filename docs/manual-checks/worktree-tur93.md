# Manual checks: tur93

TUR-93: Settings gets its own Audio section (after Speech) holding the audio
retention row and the Bluetooth mic row. `mic_setting.rs` and `calendar/signin.rs`
use the shared `crate::error::on_blocking_pool`. The calendar manual checks
from PR #98 are back in `worktree-tur90-part2.md`. The TUR-91 checks cite the
clean 44100 Hz then 16000 Hz pair from the owner's log.

## Run by hand

Needs the running app, so it was not run here.

1. Open Settings.
   Expect: an "Audio" section right after the speech engine card, with the
   audio retention row and "Use the Mac's own mic when Bluetooth headphones are
   connected". The "Files" section no longer has either row.
2. Flip the Bluetooth row and reopen Settings.
   Expect: the choice is kept.
