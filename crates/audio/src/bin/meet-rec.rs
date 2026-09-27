//! `meet-rec` — the standalone capture CLI.
//!
//! SPEC §4: Phase 0 needs no Tauri and no UI. This binary is how the capture
//! work gets built and tested on its own.
//!
//! Not implemented yet. It exists so `cargo run -p audio --bin meet-rec` works
//! from the first commit and the argument surface is already agreed.

use std::process::ExitCode;

const USAGE: &str = "\
meet-rec — record a meeting's microphone and system audio to WAV.

USAGE:
    meet-rec --list-devices
    meet-rec --out <DIR>

OPTIONS:
    --list-devices    Print the input and output devices meet-ai can see.
    --out <DIR>       Write mic.wav, system.wav and segments.json into DIR.
    -h, --help        Print this message.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("-h") | Some("--help") | None => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("meet-rec: `{other}` is not implemented yet.");
            eprintln!();
            eprintln!(
                "Capture lands in Phase 0. Until then this binary only proves the \
                 crate builds and runs."
            );
            ExitCode::FAILURE
        }
    }
}
