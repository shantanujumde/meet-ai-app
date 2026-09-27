// Without this, a release build on Windows pops a console window behind the app.
// v1 is macOS-only, but the attribute is free and keeps the Windows seam clean.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    meet_ai_lib::run();
}
