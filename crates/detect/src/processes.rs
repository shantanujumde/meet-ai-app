//! Which running processes mean a meeting app is open.
//!
//! The list is data, not code — `crates/detect/processes.json`, compiled in
//! with `include_str!` — so the Windows port (SPEC §8.2) adds `Zoom.exe` and
//! friends with a data edit and no new match arms.
//!
//! Each entry names the OS it is for (`"os"`, spelled the way
//! [`std::env::consts::OS`] spells it), so one file holds every platform and
//! only this machine's entries are matched (TUR-60).

use std::sync::OnceLock;

use serde::Deserialize;

/// The list as shipped. Parsed once, on first use.
const PROCESSES_JSON: &str = include_str!("../processes.json");

/// An OS a `processes.json` entry is for, spelled as [`std::env::consts::OS`]
/// spells it. Any other value fails to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

impl Os {
    /// The value for [`std::env::consts::OS`], if meet-ai knows that OS.
    pub fn from_consts(os: &str) -> Option<Self> {
        match os {
            "macos" => Some(Self::Macos),
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }
}

/// One meeting app, as `processes.json` describes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MeetingProcess {
    /// The OS this name is for.
    pub os: Os,
    /// The process name as the OS reports it, e.g. `zoom.us`. Matched exactly,
    /// ignoring case.
    pub name: String,
    /// What to call the app in the prompt, e.g. `Zoom`.
    pub label: String,
    /// The app may be open all day (Slack, Discord, and Zoom, Teams or Webex
    /// started at login), so it being open says nothing on its own. It only
    /// prompts when a call signal — a calendar event or audio activity —
    /// happened recently too. Every app in the list has it since TUR-169.
    /// See [`crate::Detector`].
    pub needs_call_signal: bool,
}

/// Every meeting app meet-ai looks for on this OS.
pub fn meeting_processes() -> &'static [MeetingProcess] {
    static LIST: OnceLock<Vec<MeetingProcess>> = OnceLock::new();
    LIST.get_or_init(|| {
        parse(PROCESSES_JSON)
            .map(|list| for_os(list, std::env::consts::OS))
            .unwrap_or_else(|error| {
                // The file is compiled in and parsed by a unit test, so this is a
                // build mistake. Detecting nothing beats taking the app down.
                tracing::error!(%error, "processes.json did not parse; process detection is off");
                Vec::new()
            })
    })
}

/// The entries of `list` for `os` (an [`std::env::consts::OS`] value).
fn for_os(list: Vec<MeetingProcess>, os: &str) -> Vec<MeetingProcess> {
    let os = Os::from_consts(os);
    list.into_iter()
        .filter(|known| Some(known.os) == os)
        .collect()
}

fn parse(raw: &str) -> Result<Vec<MeetingProcess>, serde_json::Error> {
    serde_json::from_str(raw)
}

/// The meeting app a running process is, if it is one: the exact name,
/// ignoring case. `zoom.us` matches `Zoom.us`; `Slack Helper` is not `Slack`.
pub fn find(process_name: &str) -> Option<&'static MeetingProcess> {
    find_in(meeting_processes(), process_name)
}

fn find_in<'a>(list: &'a [MeetingProcess], process_name: &str) -> Option<&'a MeetingProcess> {
    list.iter()
        .find(|known| known.name.eq_ignore_ascii_case(process_name.trim()))
}

/// This OS's process name for the app labelled `label`, e.g. `zoom.us` for
/// `Zoom` on macOS. Lets one test run on every OS, here and in the app's
/// tests. Panics on an unknown label, so a typo cannot make a negative test
/// pass without testing anything.
#[cfg(any(test, feature = "test-util"))]
pub fn name_of(label: &str) -> &'static str {
    match meeting_processes()
        .iter()
        .find(|known| known.label == label)
    {
        Some(known) => known.name.as_str(),
        None => panic!(
            "no meeting app labelled {label:?} on {}",
            std::env::consts::OS
        ),
    }
}

/// The name to show for a process name: its label if it is a known meeting
/// app, else the name itself.
pub fn label(process_name: &str) -> &str {
    find(process_name).map_or(process_name, |known| known.label.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped names for `os`, in file order.
    fn names_for(os: &str) -> Vec<String> {
        let list = parse(PROCESSES_JSON).expect("processes.json must parse");
        for_os(list, os).into_iter().map(|p| p.name).collect()
    }

    #[test]
    fn the_shipped_list_parses_and_has_the_spec_2_3_apps() {
        assert_eq!(
            names_for("macos"),
            [
                "zoom.us",
                "Microsoft Teams",
                "Webex",
                "Slack",
                "Discord",
                "WhatsApp",
                "FaceTime",
                "Telegram",
                "Signal",
            ]
        );
    }

    #[test]
    fn the_shipped_list_has_the_windows_names() {
        assert_eq!(
            names_for("windows"),
            [
                "Zoom.exe",
                "ms-teams.exe",
                "ms-teams_modulehost.exe",
                "Teams.exe",
                "Webex.exe",
                "CiscoCollabHost.exe",
                "slack.exe",
                "Discord.exe",
            ]
        );
    }

    #[test]
    fn the_shipped_list_has_the_linux_names() {
        assert_eq!(
            names_for("linux"),
            [
                "zoom",
                "teams-for-linux",
                "teams",
                "webex",
                "CiscoCollabHost",
                "slack",
                "discord",
            ]
        );
    }

    #[test]
    fn every_entry_is_for_a_known_os_and_sysinfo_can_see_its_name() {
        let list = parse(PROCESSES_JSON).expect("processes.json must parse");
        for known in &list {
            if known.os == Os::Linux {
                // Linux reports at most 15 bytes of a process name (`comm`).
                assert!(known.name.len() <= 15, "{known:?}");
            }
        }
    }

    #[test]
    fn this_machine_matches_only_its_own_entries() {
        let mine = names_for(std::env::consts::OS);
        let loaded: Vec<&str> = meeting_processes()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(loaded, mine);
        assert!(
            !loaded.is_empty(),
            "no meeting apps for {}",
            std::env::consts::OS
        );
    }

    #[test]
    fn every_teams_and_webex_process_shares_one_label() {
        let list = parse(PROCESSES_JSON).expect("processes.json must parse");
        let label = |os: &str, name: &str| {
            find_in(&for_os(list.clone(), os), name).map(|p| p.label.clone())
        };
        for (os, name) in [
            ("windows", "ms-teams.exe"),
            ("windows", "ms-teams_modulehost.exe"),
            ("windows", "Teams.exe"),
            ("linux", "teams"),
            ("linux", "teams-for-linux"),
            ("macos", "Microsoft Teams"),
        ] {
            assert_eq!(
                label(os, name).as_deref(),
                Some("Microsoft Teams"),
                "{os} {name}"
            );
        }
        assert_eq!(
            label("windows", "CiscoCollabHost.exe").as_deref(),
            Some("Webex")
        );
        assert_eq!(label("linux", "CiscoCollabHost").as_deref(), Some("Webex"));
    }

    #[test]
    fn every_app_needs_a_call_signal() {
        let list = parse(PROCESSES_JSON).expect("processes.json must parse");
        for os in ["macos", "windows", "linux"] {
            let apps = for_os(list.clone(), os);
            assert!(!apps.is_empty(), "{os}");
            // TUR-169: Zoom, Teams and Webex start at login and stay open too.
            for app in apps {
                assert!(app.needs_call_signal, "{os} {}", app.name);
            }
        }
    }

    #[test]
    fn matching_is_exact_but_ignores_case() {
        let list = for_os(parse(PROCESSES_JSON).expect("parses"), "macos");
        let find = |name: &str| find_in(&list, name);
        assert_eq!(find("zoom.us").map(|p| p.label.as_str()), Some("Zoom"));
        assert_eq!(find("ZOOM.US").map(|p| p.label.as_str()), Some("Zoom"));
        assert_eq!(
            find("microsoft teams").map(|p| p.name.as_str()),
            Some("Microsoft Teams")
        );
        assert!(find("zoom").is_none());
        assert!(find("zoom.us helper").is_none());
        assert!(find("Slack Helper (Renderer)").is_none());
        assert!(find("Finder").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn a_label_falls_back_to_the_process_name() {
        assert_eq!(label(name_of("Zoom")), "Zoom");
        assert_eq!(label("SomethingElse"), "SomethingElse");
    }

    #[test]
    fn an_unknown_os_fails_to_parse() {
        let raw = r#"[{ "os": "macOS", "name": "x", "label": "X", "needs_call_signal": false }]"#;
        assert!(parse(raw).is_err());
    }

    #[test]
    #[should_panic(expected = "no meeting app labelled")]
    fn name_of_an_unknown_label_panics() {
        name_of("Zooom");
    }

    #[test]
    fn a_new_platform_is_a_data_edit() {
        let list =
            parse(r#"[{ "os": "windows", "name": "Zoom.exe", "label": "Zoom", "needs_call_signal": false }]"#)
                .expect("parses");
        assert_eq!(
            find_in(&list, "zoom.exe").map(|p| p.label.as_str()),
            Some("Zoom")
        );
    }
}
