# Manual checks: tur93

TUR-93: Settings gets its own Audio section (after Speech) holding the audio
retention row and the Bluetooth mic row. `mic_setting.rs` and `calendar/signin.rs`
use the shared `crate::error::on_blocking_pool`. The calendar manual checks
from PR #98 are back in `worktree-tur90-part2.md`. The TUR-91 checks cite the
clean 44100 Hz then 16000 Hz pair from the owner's log.

## Run by hand

Needs the running app, so it was not run here.

1. Open Settings.
   Expect: the Audio section looks like the other sections (heading, card,
   rows). Where it sits is checked by `src/routes/Settings.test.tsx`.
2. Flip the Bluetooth row and reopen Settings.
   Expect: the choice is kept.
