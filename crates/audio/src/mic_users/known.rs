//! The built-in table of apps we know by name (TUR-142): call apps, browsers,
//! and the dictation and audio-routing tools that use a mic without being on
//! a call (TUR-141 decision 14).
//!
//! An entry matches a [`Named`] process by bundle id (the id itself, or the id
//! followed by `.`, compared without case) or by name (the app folder, sound
//! server label or program name, compared without case). The first match wins
//! and gives the app's UI name and [`AppKind`]. A macOS daemon outside any
//! app bundle that no entry names is [`AppKind::IgnoredSystem`]; anything
//! else is [`AppKind::Other`] under the name the OS gave.
//!
//! BlackHole (decision 14) is an audio driver, not a process, so it never
//! shows up in a process list and needs no entry.

// Adapted from github.com/island-io/mila/Mila/Audio/MeetingDetector.swift @ 605babbd5c2639841fc4ea366f9d3cf9f4a21118 (Apache-2.0)
// Adapted from github.com/fastrepl/anarlog/crates/detect/src/list/macos.rs @ 259a04ee2e1447dfed150ed08f0a1bb69909b836 (MIT)

use super::naming::Named;

/// What sort of app is using the mic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppKind {
    /// An app made for calls: Zoom, Teams, WhatsApp, FaceTime.
    CallApp,
    /// A web browser: the call is in one of its tabs (Meet, a Slack huddle).
    Browser,
    /// Anything else using the mic: a recorder, a game, an unknown app.
    Other,
    /// Never a call: dictation, voice assistants, mic and audio-routing
    /// tools (Krisp, Rogue Amoeba's apps, Superwhisper, Siri).
    IgnoredSystem,
}

struct Known {
    /// The UI name, or `None` to keep the name the OS gave (one entry for
    /// several apps of one maker).
    display: Option<&'static str>,
    kind: AppKind,
    /// Lower-case bundle ids (macOS).
    bundles: &'static [&'static str],
    /// Lower-case names: the app folder without `.app`, the program without
    /// `.exe`, the Linux binary.
    names: &'static [&'static str],
}

const fn call(
    display: &'static str,
    bundles: &'static [&'static str],
    names: &'static [&'static str],
) -> Known {
    Known {
        display: Some(display),
        kind: AppKind::CallApp,
        bundles,
        names,
    }
}

const fn browser(
    display: &'static str,
    bundles: &'static [&'static str],
    names: &'static [&'static str],
) -> Known {
    Known {
        display: Some(display),
        kind: AppKind::Browser,
        bundles,
        names,
    }
}

const fn ignored(
    display: Option<&'static str>,
    bundles: &'static [&'static str],
    names: &'static [&'static str],
) -> Known {
    Known {
        display,
        kind: AppKind::IgnoredSystem,
        bundles,
        names,
    }
}

/// Bundle ids from mila's app table and anarlog's meeting-app list; program
/// names from `crates/detect/processes.json`; Apple's call daemons from
/// anarlog's FaceTime labels (TUR-141 decision 18).
const KNOWN: &[Known] = &[
    call("Zoom", &["us.zoom"], &["zoom.us", "zoom"]),
    call(
        "Microsoft Teams",
        &["com.microsoft.teams2", "com.microsoft.teams"],
        &[
            "microsoft teams",
            "ms-teams",
            "ms-teams_modulehost",
            "teams",
            "teams-for-linux",
        ],
    ),
    call(
        "Webex",
        &["cisco-systems.spark"],
        &["webex", "ciscocollabhost"],
    ),
    call("Slack", &[], &["slack"]),
    call("Discord", &[], &["discord"]),
    call("WhatsApp", &["net.whatsapp.whatsapp"], &["whatsapp"]),
    call("Skype", &[], &["skype"]),
    call("Telegram", &[], &["telegram"]),
    call("Signal", &[], &["signal"]),
    call(
        "FaceTime",
        &["com.apple.avconferenced", "com.apple.facetime"],
        &["avconferenced", "facetime"],
    ),
    call(
        "Phone call",
        &["com.apple.telephonyutilities"],
        &["callservicesd"],
    ),
    browser(
        "Google Chrome",
        &["com.google.chrome"],
        &["google chrome", "chrome", "google-chrome"],
    ),
    browser("Arc", &["company.thebrowser.browser"], &["arc"]),
    browser("Helium", &[], &["helium"]),
    browser("Safari", &["com.apple.safari"], &["safari"]),
    browser("Firefox", &["org.mozilla.firefox"], &["firefox"]),
    browser(
        "Microsoft Edge",
        &["com.microsoft.edgemac"],
        &["microsoft edge", "msedge"],
    ),
    browser("Brave", &["com.brave.browser"], &["brave browser", "brave"]),
    browser(
        "Chromium",
        &["org.chromium.chromium"],
        &["chromium", "chromium-browser"],
    ),
    browser("Island", &["io.island.island"], &["island"]),
    browser("Vivaldi", &[], &["vivaldi"]),
    browser("Opera", &[], &["opera"]),
    ignored(Some("Krisp"), &[], &["krisp"]),
    ignored(
        None,
        &["com.rogueamoeba"],
        &[
            "loopback",
            "audio hijack",
            "soundsource",
            "piezo",
            "farrago",
        ],
    ),
    ignored(Some("Superwhisper"), &[], &["superwhisper"]),
    ignored(Some("Wispr Flow"), &[], &["wispr flow"]),
    ignored(Some("MacWhisper"), &[], &["macwhisper"]),
    ignored(Some("Raycast"), &["com.raycast.macos"], &["raycast"]),
    ignored(
        Some("Siri"),
        &[
            "com.apple.corespeech",
            "com.apple.siri",
            "com.apple.assistantd",
        ],
        &["corespeechd", "siri"],
    ),
    // Another copy of meet-ai (a dev build next to the installed app). This
    // one's own pid is dropped before naming.
    ignored(Some("meet-ai"), &["pro.saleschat.meetai"], &["meet-ai"]),
];

/// The UI name and kind for `named`.
pub(super) fn classify(named: &Named) -> (String, AppKind) {
    let bundle = named.bundle.as_deref().map(str::to_ascii_lowercase);
    let names = [named.name.to_lowercase(), named.exe.to_lowercase()];
    let hit = KNOWN.iter().find(|known| {
        let by_bundle = bundle
            .as_deref()
            .is_some_and(|id| known.bundles.iter().any(|b| bundle_matches(id, b)));
        by_bundle
            || names
                .iter()
                .any(|name| known.names.contains(&name.as_str()))
    });
    match hit {
        Some(known) => (known.display.unwrap_or(&named.name).to_string(), known.kind),
        None if named.system => (named.name.clone(), AppKind::IgnoredSystem),
        None => (named.name.clone(), AppKind::Other),
    }
}

/// `id` is `known` or one of its sub-ids (`us.zoom.xos` under `us.zoom`),
/// but not a longer name that merely starts the same (`us.zoomer`).
fn bundle_matches(id: &str, known: &str) -> bool {
    id.strip_prefix(known)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(bundle: Option<&str>, name: &str, exe: &str) -> Named {
        Named {
            id: bundle.unwrap_or(exe).to_string(),
            name: name.to_string(),
            bundle: bundle.map(str::to_string),
            exe: exe.to_string(),
            system: false,
        }
    }

    #[test]
    fn bundle_ids_match_whole_dot_separated_parts() {
        assert!(bundle_matches("us.zoom.xos", "us.zoom"));
        assert!(bundle_matches("us.zoom", "us.zoom"));
        assert!(!bundle_matches("us.zoomer", "us.zoom"));
        assert!(!bundle_matches(
            "com.microsoft.teams2",
            "com.microsoft.teams"
        ));
    }

    #[test]
    fn the_table_names_and_kinds_apps() {
        let cases = [
            (
                named(Some("us.zoom.xos"), "zoom.us", "zoom.us"),
                "Zoom",
                AppKind::CallApp,
            ),
            (
                named(Some("com.microsoft.teams2"), "Microsoft Teams", "MSTeams"),
                "Microsoft Teams",
                AppKind::CallApp,
            ),
            (
                named(None, "discord", "discord"),
                "Discord",
                AppKind::CallApp,
            ),
            (
                named(
                    Some("COM.GOOGLE.CHROME"),
                    "Google Chrome",
                    "Google Chrome Helper",
                ),
                "Google Chrome",
                AppKind::Browser,
            ),
            (
                named(None, "firefox", "firefox"),
                "Firefox",
                AppKind::Browser,
            ),
            (
                named(
                    Some("com.rogueamoeba.audiohijack"),
                    "Audio Hijack",
                    "Audio Hijack",
                ),
                "Audio Hijack",
                AppKind::IgnoredSystem,
            ),
            (
                named(None, "Krisp", "krisp"),
                "Krisp",
                AppKind::IgnoredSystem,
            ),
            (
                named(None, "Wispr Flow", "Wispr Flow"),
                "Wispr Flow",
                AppKind::IgnoredSystem,
            ),
            (
                named(None, "superwhisper", "superwhisper"),
                "Superwhisper",
                AppKind::IgnoredSystem,
            ),
            (
                named(None, "MacWhisper", "MacWhisper"),
                "MacWhisper",
                AppKind::IgnoredSystem,
            ),
            (
                named(None, "Recorder", "Recorder"),
                "Recorder",
                AppKind::Other,
            ),
            (
                named(Some("com.apple.CoreSpeech"), "corespeechd", "corespeechd"),
                "Siri",
                AppKind::IgnoredSystem,
            ),
            (
                named(Some("net.whatsapp.WhatsApp.ServiceExtension"), "x", "x"),
                "WhatsApp",
                AppKind::CallApp,
            ),
        ];
        for (named, name, kind) in cases {
            assert_eq!(classify(&named), (name.to_string(), kind), "{named:?}");
        }
    }

    #[test]
    fn an_unknown_system_daemon_is_ignored_system() {
        let daemon = Named {
            system: true,
            ..named(Some("com.apple.audiomxd"), "audiomxd", "audiomxd")
        };
        assert_eq!(
            classify(&daemon),
            ("audiomxd".to_string(), AppKind::IgnoredSystem)
        );
        let facetime = Named {
            system: true,
            ..named(
                Some("com.apple.avconferenced"),
                "avconferenced",
                "avconferenced",
            )
        };
        assert_eq!(classify(&facetime).1, AppKind::CallApp);
    }
}
