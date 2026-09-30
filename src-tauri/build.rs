fn main() {
    link_clang_runtime();
    tauri_build::build();
}

/// Link clang's compiler runtime into the app on macOS.
///
/// whisper.cpp's Metal backend (via `stt`, in the app since TUR-96) guards
/// newer Metal calls with `@available`, which clang lowers to a call to
/// `__isPlatformVersionAtLeast` in `libclang_rt.osx.a`. rustc links with
/// `-nodefaultlibs`, so that archive is never passed and the release link
/// fails with an undefined `___isPlatformVersionAtLeast`. Ask the same clang
/// that `cc` resolves to where its runtime lives, rather than hard-coding a
/// Command Line Tools or Xcode path that changes with every toolchain update.
fn link_clang_runtime() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let output = std::process::Command::new("clang")
        .arg("--print-runtime-dir")
        .output()
        .expect("clang must be on PATH to build meet-ai for macOS");
    let dir = String::from_utf8(output.stdout).expect("clang printed a non-UTF-8 runtime dir");
    let dir = dir.trim();
    println!("cargo:rustc-link-search=native={dir}");
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
