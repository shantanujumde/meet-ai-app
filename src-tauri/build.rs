fn main() {
    let mut attributes = tauri_build::Attributes::new();
    if windows_msvc() {
        // tauri-build links its app manifest into the app binary only, so on
        // Windows every test binary of this crate died at start with
        // STATUS_ENTRYPOINT_NOT_FOUND (tauri-apps/tauri#13419): without the
        // manifest's Common Controls v6 dependency, comctl32 lacks the entry
        // points tauri imports. Link the same manifest into every artifact
        // instead, tests included (TUR-36).
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        embed_manifest_everywhere();
    }
    if let Err(error) = tauri_build::try_build(attributes) {
        panic!("error found during tauri-build: {error:#}");
    }
}

/// Building for Windows with the MSVC linker. Read from Cargo's env, not
/// `cfg!`, because a build script runs on the host, not the target.
fn windows_msvc() -> bool {
    let var = |name| std::env::var(name).unwrap_or_default();
    var("CARGO_CFG_TARGET_OS") == "windows" && var("CARGO_CFG_TARGET_ENV") == "msvc"
}

// Adapted from github.com/tauri-apps/tauri/crates/tauri/build.rs @ 6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a (Apache-2.0 OR MIT)
fn embed_manifest_everywhere() {
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    // `rustc-link-arg`, not `-bins`: binaries, the cdylib and test binaries.
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
