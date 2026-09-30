//! Link clang's compiler runtime wherever whisper is linked on macOS.
//!
//! whisper.cpp's Metal backend guards newer Metal calls with `@available`,
//! which clang lowers to a call to `__isPlatformVersionAtLeast` in
//! `libclang_rt.osx.a`. rustc links with `-nodefaultlibs`, so that archive is
//! never passed, and any release binary that pulls in the Metal objects fails
//! with an undefined `___isPlatformVersionAtLeast` — the app since TUR-96, and
//! equally `live_replay` or anything else built on this crate. Debug test
//! binaries never reference those objects, which is why `cargo test` stayed
//! green while `pnpm tauri build` broke.
//!
//! The directive lives here, next to the dependency that needs it, so every
//! binary that links `stt` gets it. The runtime dir is asked of the compiler
//! the link will use — `$CC` if set, else `cc`, rustc's default linker on
//! macOS — rather than hard-coding a Command Line Tools or Xcode path that
//! moves with every toolchain update.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CC");

    // whisper-rs is a macOS-only dependency (Cargo.toml), so is its runtime.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let output = Command::new(&compiler)
        .arg("--print-runtime-dir")
        .output()
        .unwrap_or_else(|error| panic!("could not run `{compiler} --print-runtime-dir`: {error}"));
    if !output.status.success() {
        panic!(
            "`{compiler} --print-runtime-dir` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let dir = String::from_utf8(output.stdout)
        .unwrap_or_else(|_| panic!("`{compiler} --print-runtime-dir` printed non-UTF-8"));
    let dir = dir.trim();
    if dir.is_empty() {
        panic!("`{compiler} --print-runtime-dir` printed nothing");
    }

    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
