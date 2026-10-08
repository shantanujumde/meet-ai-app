//! Which apps are using a mic, every 2 s (TUR-142).
//!
//! Prints `audio::mic_users::mic_users` every 2 s, the detection loop's
//! cadence, for N seconds (default 60), then exits. For the manual checks in
//! `docs/manual-checks/worktree-tur142.md`: start a call, watch the app
//! appear; hang up, watch it go within one read.
//!
//! ```sh
//! cargo run -p audio --example mic_users 60
//! ```
//!
//! Stream-list and property reads only: no stream, no permission prompt.

use std::time::{Duration, Instant};

use audio::mic_users::{MicUsers, mic_users};

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(60);
    let interval = Duration::from_secs(2);
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        match mic_users() {
            MicUsers::Supported(apps) if apps.is_empty() => println!("nobody else"),
            MicUsers::Supported(apps) => {
                let line: Vec<String> = apps
                    .iter()
                    .map(|app| {
                        format!(
                            "{} [{:?}] id={} pid={} playing={}",
                            app.name, app.kind, app.id, app.pid, app.playing
                        )
                    })
                    .collect();
                println!("{}", line.join(" | "));
            }
            MicUsers::NotSupported => println!("not supported here"),
        }
        std::thread::sleep(interval);
    }
}
