//! Idle-poll cost of the audio-activity reads (TUR-31).
//!
//! Reads `audio::macos::activity::read` every 2 s — the detection loop's
//! cadence — for N seconds (default 90), printing each reading, then exits.
//! Measure it from outside so the number includes everything the process did:
//!
//! ```sh
//! cargo build --release -p audio --example activity_poll
//! /usr/bin/time -l target/release/examples/activity_poll 90
//! ```
//!
//! CPU % = (user + sys) / real. Property reads only: no stream, no permission
//! prompt.

#[cfg(target_os = "macos")]
fn main() {
    use std::time::{Duration, Instant};

    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(90);
    let interval = Duration::from_secs(2);
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut reads = 0u32;
    while Instant::now() < end {
        match audio::macos::activity::read() {
            Ok(reading) => println!(
                "input_running={} output_running={}",
                reading.input_running, reading.output_running
            ),
            Err(error) => eprintln!("read failed: {error}"),
        }
        reads += 1;
        std::thread::sleep(interval);
    }
    println!("{reads} reads in {seconds} s");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("activity_poll reads Core Audio and runs on macOS only");
}
