//! Which apps are using a mic right now, by name (TUR-142).
//!
//! [`crate::activity::device_activity`] only answers "is the mic busy", and
//! while meet-ai records, its own stream makes that always yes. This module
//! answers *who*: one [`MicApp`] per app using a mic, never meet-ai itself,
//! with a name a person would recognise ("Google Chrome", not
//! `com.google.Chrome.helper`) and a rough [`AppKind`] for the call-detection
//! rules built on it (TUR-143, TUR-144).
//!
//! The OS reads live in the platform files: the Core Audio process list on
//! macOS 14 and later (`macos/activity.rs`), WASAPI capture sessions on
//! Windows and PulseAudio source outputs on Linux (`platform/*/activity.rs`).
//! Each hands the processes it found to [`name_apps`] as [`ProcessFacts`]; the
//! naming and filtering here are OS-free and tested on every OS with fake
//! process data. Where the OS cannot list processes (macOS older than 14, no
//! sound server) the answer is [`MicUsers::NotSupported`] and callers keep the
//! yes/no signal.
//!
//! This is a poll: nothing here registers for change notifications (Core
//! Audio does not send them for `IsRunningInput`). The detection loop decides
//! how often to ask.

mod known;
mod naming;

pub use known::AppKind;
#[allow(unused_imports)] // only the macOS reader uses it
pub(crate) use naming::outermost_app_dir;

/// The answer to "which apps are using a mic".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MicUsers {
    /// The OS listed its processes. Empty when nobody but meet-ai (or nobody
    /// at all) is using a mic.
    Supported(Vec<MicApp>),
    /// This OS cannot say which processes use a mic: macOS older than 14,
    /// no sound server running, or the read failed. Not an error for the
    /// user; callers fall back to [`crate::activity::device_activity`].
    NotSupported,
}

/// One app using a mic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicApp {
    /// The process that has the mic open. For a browser this is the helper
    /// (or audio service) process, not the browser's main process. `0` when
    /// the OS does not say (a PulseAudio stream without a process id).
    pub pid: u32,
    /// A stable id for the app, for "do not ask again" lists: the bundle id
    /// without a `.helper…` suffix on macOS (`com.google.Chrome`), the
    /// lower-cased program name elsewhere (`chrome.exe`, `zoom`).
    pub id: String,
    /// The app's name for the UI: "Zoom", "Google Chrome", "FaceTime".
    pub name: String,
    /// What sort of app it is, from a built-in table.
    pub kind: AppKind,
    /// The same app is also playing audio (Core Audio's `IsRunningOutput`, an
    /// active render session or sink input from the same process).
    pub playing: bool,
}

/// Which apps are using a mic right now, without meet-ai's own recording.
/// Property and stream-list reads only: no stream is opened, so there is no
/// permission prompt and no mic indicator.
pub fn mic_users() -> MicUsers {
    crate::platform::mic_users()
}

/// One process, as much as the OS tells about it. Every field but `pid` may
/// be missing; [`name_apps`] uses whatever is there.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessFacts {
    pub pid: u32,
    /// macOS: the bundle id Core Audio reports (`com.google.Chrome.helper`).
    pub bundle_id: Option<String>,
    /// The executable's full path (`proc_pidpath` on macOS, `sysinfo` on
    /// Windows).
    pub path: Option<String>,
    /// The program's file name: `chrome.exe`, `zoom`, `avconferenced`.
    pub exe: String,
    /// What the app calls itself, when the OS says (PulseAudio's
    /// `application.name`).
    pub label: Option<String>,
    /// The command line after the program, for a Chromium child's
    /// `--type=utility`. Windows only.
    pub args: Vec<String>,
    /// The parent process, when known.
    pub parent: Option<u32>,
    /// macOS: the process responsible for this one, when it is not itself
    /// (Safari, or meet-ai, for a `com.apple.WebKit.GPU` process).
    pub responsible: Option<u32>,
    /// The process is also playing audio.
    pub output: bool,
}

/// The apps behind `users`, the processes the OS says have a mic open.
///
/// Drops `own_pid` (meet-ai) and every process it is responsible for or
/// started (our WebKit child). Names each of the rest with
/// [`naming::resolve`], which may look up a related process (a WebKit
/// process's responsible app, a Chromium child's parent) through `related`.
/// Several processes of one app become one [`MicApp`]. Sorted by name.
pub fn name_apps(
    users: Vec<ProcessFacts>,
    own_pid: u32,
    related: impl Fn(u32) -> Option<ProcessFacts>,
) -> Vec<MicApp> {
    let mut apps: Vec<MicApp> = Vec::new();
    for process in users {
        if naming::is_ours(&process, own_pid) {
            continue;
        }
        let Some(named) = naming::resolve(&process, own_pid, &related) else {
            continue;
        };
        let (name, kind) = known::classify(&named);
        match apps.iter_mut().find(|app| app.id == named.id) {
            Some(app) => app.playing |= process.output,
            None => apps.push(MicApp {
                pid: process.pid,
                id: named.id,
                name,
                kind,
                playing: process.output,
            }),
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: u32 = 4242;

    fn mac(pid: u32, bundle: &str, path: &str) -> ProcessFacts {
        ProcessFacts {
            pid,
            bundle_id: Some(bundle.to_string()),
            path: Some(path.to_string()),
            exe: path.rsplit('/').next().unwrap_or_default().to_string(),
            ..ProcessFacts::default()
        }
    }

    fn nobody(_: u32) -> Option<ProcessFacts> {
        None
    }

    fn names(apps: &[MicApp]) -> Vec<(&str, AppKind)> {
        apps.iter().map(|a| (a.name.as_str(), a.kind)).collect()
    }

    /// Answers on every OS: a list, or "not supported", never a panic.
    #[test]
    fn mic_users_answers_on_this_machine() {
        match mic_users() {
            MicUsers::Supported(apps) => {
                assert!(apps.iter().all(|a| a.pid != std::process::id()));
            }
            MicUsers::NotSupported => {}
        }
    }

    #[test]
    fn our_own_recording_and_webkit_child_never_show() {
        let users = vec![
            mac(
                OURS,
                "pro.saleschat.meetai",
                "/Applications/meet-ai.app/Contents/MacOS/meet-ai",
            ),
            ProcessFacts {
                responsible: Some(OURS),
                ..mac(
                    77,
                    "com.apple.WebKit.GPU",
                    "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.GPU.xpc/Contents/MacOS/com.apple.WebKit.GPU",
                )
            },
            ProcessFacts {
                pid: 78,
                exe: "msedgewebview2.exe".to_string(),
                parent: Some(OURS),
                ..ProcessFacts::default()
            },
        ];
        assert!(name_apps(users, OURS, nobody).is_empty());
    }

    #[test]
    fn a_zoom_call_is_a_call_app_named_zoom() {
        let users = vec![mac(
            10,
            "us.zoom.xos",
            "/Applications/zoom.us.app/Contents/MacOS/zoom.us",
        )];
        let apps = name_apps(users, OURS, nobody);
        assert_eq!(
            apps,
            [MicApp {
                pid: 10,
                id: "us.zoom.xos".to_string(),
                name: "Zoom".to_string(),
                kind: AppKind::CallApp,
                playing: false,
            }]
        );
    }

    #[test]
    fn browser_helpers_show_as_their_browser() {
        let users = vec![
            mac(
                11,
                "com.google.Chrome.helper",
                "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/140.0.0.0/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper",
            ),
            mac(
                12,
                "company.thebrowser.browser.helper",
                "/Applications/Arc.app/Contents/Frameworks/ArcCore.framework/Helpers/Browser Helper.app/Contents/MacOS/Browser Helper",
            ),
            mac(
                13,
                "net.imput.helium.helper",
                "/Applications/Helium.app/Contents/Frameworks/Helium Framework.framework/Helpers/Helium Helper.app/Contents/MacOS/Helium Helper",
            ),
        ];
        let apps = name_apps(users, OURS, nobody);
        assert_eq!(
            names(&apps),
            [
                ("Arc", AppKind::Browser),
                ("Google Chrome", AppKind::Browser),
                ("Helium", AppKind::Browser),
            ]
        );
        let chrome = apps.iter().find(|a| a.name == "Google Chrome");
        assert_eq!(chrome.map(|a| a.id.as_str()), Some("com.google.Chrome"));
    }

    #[test]
    fn two_helpers_of_one_browser_are_one_app() {
        let path = "/Applications/Google Chrome.app/Contents/Frameworks/x/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        let users = vec![
            mac(11, "com.google.Chrome.helper", path),
            ProcessFacts {
                output: true,
                ..mac(14, "com.google.Chrome.helper.renderer", path)
            },
        ];
        let apps = name_apps(users, OURS, nobody);
        assert_eq!(apps.len(), 1, "{apps:?}");
        assert_eq!(apps[0].pid, 11);
        assert!(apps[0].playing, "the second helper's output counts");
    }

    #[test]
    fn webkit_shows_as_its_responsible_app_or_safari() {
        let gpu = "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.GPU.xpc/Contents/MacOS/com.apple.WebKit.GPU";
        let safari = mac(
            50,
            "com.apple.Safari",
            "/System/Cryptexes/App/System/Applications/Safari.app/Contents/MacOS/Safari",
        );
        let users = vec![ProcessFacts {
            responsible: Some(50),
            ..mac(20, "com.apple.WebKit.GPU", gpu)
        }];
        let apps = name_apps(users.clone(), OURS, |pid| {
            (pid == 50).then(|| safari.clone())
        });
        assert_eq!(names(&apps), [("Safari", AppKind::Browser)]);
        assert_eq!(apps[0].id, "com.apple.Safari");

        // The responsible lookup is missing (the symbol did not load) or
        // finds nothing: still Safari.
        let unknown = vec![mac(21, "com.apple.WebKit.GPU", gpu)];
        assert_eq!(
            names(&name_apps(unknown, OURS, nobody)),
            [("Safari", AppKind::Browser)]
        );
        assert_eq!(
            names(&name_apps(users, OURS, nobody)),
            [("Safari", AppKind::Browser)]
        );

        // A WebKit view in some other app is that app.
        let notes = mac(
            60,
            "com.example.Notes",
            "/Applications/Notes.app/Contents/MacOS/Notes",
        );
        let users = vec![ProcessFacts {
            responsible: Some(60),
            ..mac(22, "com.apple.WebKit.GPU", gpu)
        }];
        let apps = name_apps(users, OURS, |pid| (pid == 60).then(|| notes.clone()));
        assert_eq!(names(&apps), [("Notes", AppKind::Other)]);
    }

    #[test]
    fn apple_call_daemons_are_facetime_and_phone_call() {
        let users = vec![
            mac(30, "com.apple.avconferenced", "/usr/libexec/avconferenced"),
            mac(
                31,
                "com.apple.TelephonyUtilities.callservicesd",
                "/System/Library/PrivateFrameworks/TelephonyUtilities.framework/callservicesd",
            ),
        ];
        assert_eq!(
            names(&name_apps(users, OURS, nobody)),
            [
                ("FaceTime", AppKind::CallApp),
                ("Phone call", AppKind::CallApp)
            ]
        );
        // Known by the program alone, with no bundle id.
        let bare = vec![ProcessFacts {
            pid: 32,
            path: Some("/usr/libexec/avconferenced".to_string()),
            exe: "avconferenced".to_string(),
            ..ProcessFacts::default()
        }];
        assert_eq!(
            names(&name_apps(bare, OURS, nobody)),
            [("FaceTime", AppKind::CallApp)]
        );
    }

    #[test]
    fn dictation_and_audio_routing_apps_are_ignored_system() {
        let users = vec![
            mac(
                40,
                "com.raycast.macos",
                "/Applications/Raycast.app/Contents/MacOS/Raycast",
            ),
            mac(
                41,
                "com.apple.CoreSpeech",
                "/System/Library/PrivateFrameworks/CoreSpeech.framework/corespeechd",
            ),
            mac(
                42,
                "com.rogueamoeba.Loopback",
                "/Applications/Loopback.app/Contents/MacOS/Loopback",
            ),
        ];
        let apps = name_apps(users, OURS, nobody);
        assert!(
            apps.iter().all(|a| a.kind == AppKind::IgnoredSystem),
            "{apps:?}"
        );
        assert_eq!(apps.len(), 3);
    }

    #[test]
    fn an_unknown_app_is_other_with_its_own_name() {
        let users = vec![mac(
            43,
            "com.example.recorder",
            "/Applications/Recorder.app/Contents/MacOS/Recorder",
        )];
        assert_eq!(
            names(&name_apps(users, OURS, nobody)),
            [("Recorder", AppKind::Other)]
        );
    }

    #[test]
    fn a_whatsapp_call_is_named_from_its_app_folder() {
        let users = vec![mac(
            44,
            "net.whatsapp.WhatsApp",
            "/Applications/WhatsApp.app/Contents/MacOS/WhatsApp",
        )];
        assert_eq!(
            names(&name_apps(users, OURS, nobody)),
            [("WhatsApp", AppKind::CallApp)]
        );
    }

    #[test]
    fn chromes_windows_audio_service_shows_as_the_browser() {
        let browser = ProcessFacts {
            pid: 100,
            path: Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string()),
            exe: "chrome.exe".to_string(),
            ..ProcessFacts::default()
        };
        let utility = ProcessFacts {
            pid: 101,
            path: browser.path.clone(),
            exe: "chrome.exe".to_string(),
            args: vec![
                "--type=utility".to_string(),
                "--utility-sub-type=audio.mojom.AudioService".to_string(),
            ],
            parent: Some(100),
            ..ProcessFacts::default()
        };
        let apps = name_apps(vec![utility], OURS, |pid| {
            (pid == 100).then(|| browser.clone())
        });
        assert_eq!(names(&apps), [("Google Chrome", AppKind::Browser)]);
        assert_eq!(apps[0].id, "chrome.exe");
        assert_eq!(apps[0].pid, 101);
    }

    #[test]
    fn windows_and_linux_programs_are_named_by_the_table_or_the_os() {
        let users = vec![
            ProcessFacts {
                pid: 1,
                exe: "Zoom.exe".to_string(),
                ..ProcessFacts::default()
            },
            ProcessFacts {
                pid: 2,
                exe: "msedge.exe".to_string(),
                ..ProcessFacts::default()
            },
            ProcessFacts {
                pid: 3,
                exe: "teams-for-linux".to_string(),
                label: Some("Teams for Linux".to_string()),
                ..ProcessFacts::default()
            },
            ProcessFacts {
                pid: 4,
                exe: "obs".to_string(),
                label: Some("OBS Studio".to_string()),
                ..ProcessFacts::default()
            },
            ProcessFacts {
                pid: 5,
                exe: "recorder.exe".to_string(),
                ..ProcessFacts::default()
            },
        ];
        let apps = name_apps(users, OURS, nobody);
        assert_eq!(
            names(&apps),
            [
                ("Microsoft Edge", AppKind::Browser),
                ("Microsoft Teams", AppKind::CallApp),
                ("OBS Studio", AppKind::Other),
                ("Zoom", AppKind::CallApp),
                ("recorder", AppKind::Other),
            ]
        );
        let zoom = apps.iter().find(|a| a.name == "Zoom");
        assert_eq!(zoom.map(|a| a.id.as_str()), Some("zoom.exe"));
    }

    #[test]
    fn a_process_with_nothing_known_is_still_listed_by_pid() {
        let users = vec![ProcessFacts {
            pid: 9,
            ..ProcessFacts::default()
        }];
        let apps = name_apps(users, OURS, nobody);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "pid:9");
        assert_eq!(apps[0].kind, AppKind::Other);
    }
}
