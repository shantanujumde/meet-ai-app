//! Turning one mic-using process into the app a person would name (TUR-142).
//!
//! Pure functions over [`ProcessFacts`], so every rule is tested on every OS:
//!
//! * A process inside an app bundle is that bundle's outermost `X.app`:
//!   Chrome's `Google Chrome Helper.app` deep inside `Google Chrome.app` is
//!   "Google Chrome". Its id is the bundle id without the `.helper…` suffix.
//! * A WebKit process (`com.apple.WebKit.GPU`) belongs to whichever app is
//!   responsible for it: Safari, meet-ai's own window (skipped), or another
//!   app's web view. With no answer it is Safari.
//! * A Chromium child on Windows (`--type=utility`, the audio service) is its
//!   parent, the browser; a WebView2 runtime process is the app hosting it.
//!
//! [`super::known`] then puts the table's name and kind on the result.

// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/macos.rs @ 259a04ee2e1447dfed150ed08f0a1bb69909b836 (MIT)

use super::ProcessFacts;

/// How many parents (or responsible processes) to follow at most, so a
/// cycle in what the OS reports can never loop forever.
const MAX_HOPS: usize = 3;

/// One process, named: what [`super::known::classify`] reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Named {
    /// See [`super::MicApp::id`].
    pub id: String,
    /// The name from the OS: the outermost app folder, the sound server's
    /// label or the program name.
    pub name: String,
    /// The bundle id without `.helper…`, when there is one.
    pub bundle: Option<String>,
    /// The program name without `.exe`.
    pub exe: String,
    /// A macOS system program: anything under `/System/Library/` (daemons,
    /// and system UI such as Control Center), or outside any app bundle in
    /// `/System/` or `/usr/libexec/` and the like. Never an app on a call unless the table says
    /// so (FaceTime's `avconferenced`). Apple's own apps, in
    /// `/System/Applications/`, are not system programs.
    pub system: bool,
}

/// meet-ai itself, or a process it is responsible for (its WebKit child on
/// macOS) or started (its WebView2 runtime on Windows).
pub(super) fn is_ours(process: &ProcessFacts, own_pid: u32) -> bool {
    process.pid == own_pid
        || process.responsible == Some(own_pid)
        || process.parent == Some(own_pid)
}

/// The app behind `process`, or `None` when it turns out to be meet-ai's
/// (a host process found through `related` is ours).
pub(super) fn resolve(
    process: &ProcessFacts,
    own_pid: u32,
    related: &impl Fn(u32) -> Option<ProcessFacts>,
) -> Option<Named> {
    if is_webkit(process) {
        return match host(process, process.responsible, related) {
            Some(host) if is_ours(&host, own_pid) => None,
            Some(host) if !is_webkit(&host) => Some(plain(&host)),
            _ => Some(safari()),
        };
    }
    let mut current = process.clone();
    for _ in 0..MAX_HOPS {
        if !is_chromium_child(&current) && !is_webview2_runtime(&current) {
            break;
        }
        let Some(parent) = host(&current, current.parent, related) else {
            break;
        };
        if is_ours(&parent, own_pid) {
            return None;
        }
        current = parent;
    }
    Some(plain(&current))
}

/// The process `pid` names, through `related`, unless it is `process` itself.
fn host(
    process: &ProcessFacts,
    pid: Option<u32>,
    related: &impl Fn(u32) -> Option<ProcessFacts>,
) -> Option<ProcessFacts> {
    pid.filter(|&pid| pid != process.pid && pid != 0)
        .and_then(related)
}

/// A WebKit XPC service: `com.apple.WebKit.GPU`, `.WebContent`,
/// `.Networking`. Its own bundle id says nothing about the app using it.
fn is_webkit(process: &ProcessFacts) -> bool {
    process.bundle_id.as_deref().is_some_and(|id| {
        let id = id.to_ascii_lowercase();
        id == "com.apple.webkit" || id.starts_with("com.apple.webkit.")
    })
}

/// A Chromium (Chrome, Edge, Electron, WebView2) child process: the browser
/// starts each with `--type=renderer`, `--type=utility` and so on.
fn is_chromium_child(process: &ProcessFacts) -> bool {
    process.args.iter().any(|arg| arg.starts_with("--type="))
}

/// The WebView2 runtime's browser process, started by the app that embeds it.
fn is_webview2_runtime(process: &ProcessFacts) -> bool {
    file_name(process).eq_ignore_ascii_case("msedgewebview2.exe")
}

/// Safari, for a WebKit process nothing better is known about.
fn safari() -> Named {
    Named {
        id: "com.apple.Safari".to_string(),
        name: "Safari".to_string(),
        bundle: Some("com.apple.Safari".to_string()),
        exe: "Safari".to_string(),
        system: false,
    }
}

/// `process` named from its own facts.
fn plain(process: &ProcessFacts) -> Named {
    let bundle = process
        .bundle_id
        .as_deref()
        .map(strip_helper)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let file = file_name(process);
    let exe = strip_exe(file).to_string();
    let app = process.path.as_deref().and_then(outermost_app);
    let name = app
        .map(str::to_string)
        .or_else(|| process.label.clone().filter(|label| !label.is_empty()))
        .or_else(|| (!exe.is_empty()).then(|| exe.clone()))
        .or_else(|| bundle.clone())
        .unwrap_or_else(|| format!("pid {}", process.pid));
    let system = process.path.as_deref().is_some_and(|path| {
        path.starts_with("/System/Library/")
            || (app.is_none() && SYSTEM_FOLDERS.iter().any(|dir| path.starts_with(dir)))
    });
    let id = bundle
        .clone()
        .or_else(|| (!file.is_empty()).then(|| file.to_lowercase()))
        .unwrap_or_else(|| format!("pid:{}", process.pid));
    Named {
        id,
        name,
        bundle,
        exe,
        system,
    }
}

/// Where macOS keeps its own programs. Not `/usr/local/`: that is the user's.
const SYSTEM_FOLDERS: &[&str] = &[
    "/System/",
    "/usr/bin/",
    "/usr/libexec/",
    "/usr/sbin/",
    "/Library/Apple/",
];

/// The program's file name: `exe`, or else the last part of `path`.
fn file_name(process: &ProcessFacts) -> &str {
    if !process.exe.is_empty() {
        return &process.exe;
    }
    process
        .path
        .as_deref()
        .and_then(|path| path.rsplit(['/', '\\']).next())
        .unwrap_or_default()
}

/// `zoom.exe` to `zoom`; any other name as it is.
fn strip_exe(file: &str) -> &str {
    match file.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") && !stem.is_empty() => stem,
        _ => file,
    }
}

/// `com.google.Chrome.helper.renderer` to `com.google.Chrome`. Bundle ids
/// are ASCII, and ASCII lower-casing keeps byte offsets, so the cut is safe.
pub(super) fn strip_helper(bundle_id: &str) -> &str {
    match bundle_id.to_ascii_lowercase().find(".helper") {
        Some(at) => &bundle_id[..at],
        None => bundle_id,
    }
}

/// The name of the outermost app bundle in a macOS path, without its
/// extension: `Google Chrome` for a helper deep inside `Google Chrome.app`.
pub(super) fn outermost_app(path: &str) -> Option<&str> {
    path.split('/').find_map(app_stem)
}

/// The outermost app bundle's folder in a macOS path:
/// `/Applications/Google Chrome.app` for a helper deep inside it.
#[allow(dead_code)] // only the macOS reader uses it
pub(crate) fn outermost_app_dir(path: &str) -> Option<&str> {
    let mut end = 0;
    for part in path.split('/') {
        end += part.len();
        if app_stem(part).is_some() {
            return Some(&path[..end]);
        }
        end += 1;
    }
    None
}

/// `Google Chrome` for `Google Chrome.app`; `None` for anything else.
fn app_stem(part: &str) -> Option<&str> {
    match part.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("app") && !stem.is_empty() => Some(stem),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outermost_app_bundle_names_a_helper() {
        assert_eq!(
            outermost_app(
                "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)"
            ),
            Some("Google Chrome")
        );
        assert_eq!(
            outermost_app("/Applications/zoom.us.app/Contents/MacOS/zoom.us"),
            Some("zoom.us")
        );
        assert_eq!(outermost_app("/usr/libexec/avconferenced"), None);
        assert_eq!(outermost_app("/Applications/.app/x"), None);
        assert_eq!(outermost_app(""), None);
        assert_eq!(
            outermost_app_dir(
                "/Applications/Google Chrome.app/Contents/Frameworks/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper"
            ),
            Some("/Applications/Google Chrome.app")
        );
        assert_eq!(outermost_app_dir("/usr/libexec/avconferenced"), None);
    }

    #[test]
    fn a_program_outside_any_app_in_a_system_folder_is_a_daemon() {
        let at = |path: &str| {
            plain(&ProcessFacts {
                pid: 1,
                path: Some(path.to_string()),
                ..ProcessFacts::default()
            })
            .system
        };
        assert!(at("/usr/libexec/audiomxd"));
        assert!(at(
            "/System/Library/PrivateFrameworks/HearingCore.framework/heard"
        ));
        assert!(at(
            "/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter"
        ));
        assert!(!at(
            "/System/Applications/Voice Memos.app/Contents/MacOS/VoiceMemos"
        ));
        assert!(!at("/Applications/Recorder.app/Contents/MacOS/Recorder"));
        assert!(at("/usr/sbin/systemstats"));
        assert!(!at("/opt/homebrew/bin/sox"));
        assert!(!at("/usr/local/bin/sox"));
    }

    #[test]
    fn a_process_with_only_a_bundle_id_is_named_by_it() {
        let named = plain(&ProcessFacts {
            pid: 1,
            bundle_id: Some("net.example.Extension".to_string()),
            ..ProcessFacts::default()
        });
        assert_eq!(named.name, "net.example.Extension");
    }

    #[test]
    fn the_helper_suffix_comes_off_a_bundle_id() {
        assert_eq!(
            strip_helper("com.google.Chrome.helper"),
            "com.google.Chrome"
        );
        assert_eq!(
            strip_helper("com.google.Chrome.helper.renderer"),
            "com.google.Chrome"
        );
        assert_eq!(
            strip_helper("company.thebrowser.browser.Helper"),
            "company.thebrowser.browser"
        );
        assert_eq!(strip_helper("us.zoom.xos"), "us.zoom.xos");
    }

    #[test]
    fn exe_comes_off_a_windows_program_only() {
        assert_eq!(strip_exe("Zoom.exe"), "Zoom");
        assert_eq!(strip_exe("zoom.us"), "zoom.us");
        assert_eq!(strip_exe(".exe"), ".exe");
    }

    #[test]
    fn the_program_name_falls_back_to_the_path() {
        let process = ProcessFacts {
            pid: 1,
            path: Some(r"C:\Program Files\Zoom\bin\Zoom.exe".to_string()),
            ..ProcessFacts::default()
        };
        let named = plain(&process);
        assert_eq!(named.id, "zoom.exe");
        assert_eq!(named.exe, "Zoom");
    }

    #[test]
    fn a_webview2_app_is_the_app_that_hosts_the_runtime() {
        let teams = ProcessFacts {
            pid: 300,
            exe: "ms-teams.exe".to_string(),
            ..ProcessFacts::default()
        };
        let runtime = ProcessFacts {
            pid: 301,
            exe: "msedgewebview2.exe".to_string(),
            parent: Some(300),
            ..ProcessFacts::default()
        };
        let audio = ProcessFacts {
            pid: 302,
            exe: "msedgewebview2.exe".to_string(),
            args: vec!["--type=utility".to_string()],
            parent: Some(301),
            ..ProcessFacts::default()
        };
        let related = |pid| match pid {
            300 => Some(teams.clone()),
            301 => Some(runtime.clone()),
            _ => None,
        };
        let named = resolve(&audio, 4242, &related);
        assert_eq!(named.map(|n| n.id), Some("ms-teams.exe".to_string()));

        // The same chain under meet-ai's own window is ours.
        let ours = |pid| match pid {
            301 => Some(ProcessFacts {
                parent: Some(4242),
                ..runtime.clone()
            }),
            _ => None,
        };
        assert_eq!(resolve(&audio, 4242, &ours), None);
    }

    #[test]
    fn a_parent_cycle_stops() {
        let looped = ProcessFacts {
            pid: 5,
            exe: "chrome.exe".to_string(),
            args: vec!["--type=utility".to_string()],
            parent: Some(6),
            ..ProcessFacts::default()
        };
        let other = ProcessFacts {
            pid: 6,
            parent: Some(5),
            ..looped.clone()
        };
        let related = |pid| match pid {
            5 => Some(looped.clone()),
            6 => Some(other.clone()),
            _ => None,
        };
        assert_eq!(
            resolve(&looped, 4242, &related).map(|n| n.id),
            Some("chrome.exe".to_string())
        );
    }
}
