//! Compiles `src/bin/fake_cli.rs` into `OUT_DIR` with a bare `rustc`, so
//! `test_support::fake_cli_path` names a ready program in every test that
//! uses it, in any crate (TUR-54). A `[[bin]]` alone would not do: cargo only
//! builds a package's own binaries for that package's integration tests.
//!
//! The program has no dependencies, so one `rustc` call is the whole build.
//! When the target is not the host (`just check-windows` checks the Windows
//! target from a Mac, with no linker for it), nothing is compiled and
//! `fake_cli_path` panics if called; `cargo check` never runs it.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let source = "src/bin/fake_cli.rs";
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let target = std::env::var("TARGET").expect("cargo sets TARGET");
    let host = std::env::var("HOST").expect("cargo sets HOST");
    let exe_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let exe = out_dir.join(format!("fake-cli{exe_suffix}"));

    if target != host {
        println!("cargo:rustc-env=FAKE_CLI_PATH=");
        return;
    }

    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    // Keep in step with `edition` in the workspace Cargo.toml; cargo does not
    // pass the edition to build scripts.
    let status = Command::new(rustc)
        .args([
            "--edition",
            "2024",
            "--crate-name",
            "fake_cli",
            "--target",
            &target,
        ])
        .arg("-o")
        .arg(&exe)
        .arg(source)
        .status()
        .expect("could not run rustc for fake-cli");
    assert!(status.success(), "rustc could not build fake-cli: {status}");
    println!("cargo:rustc-env=FAKE_CLI_PATH={}", exe.display());
}
