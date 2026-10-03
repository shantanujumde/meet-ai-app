//! Which running processes mean a meeting app is open.
//!
//! The list is data, not code — `crates/detect/processes.json`, compiled in
//! with `include_str!` — so the Windows port (SPEC §8.2) adds `Zoom.exe` and
//! friends with a data edit and no new match arms.

use std::sync::OnceLock;

use serde::Deserialize;

/// The list as shipped. Parsed once, on first use.
const PROCESSES_JSON: &str = include_str!("../processes.json");

/// One meeting app, as `processes.json` describes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MeetingProcess {
    /// The process name as the OS reports it, e.g. `zoom.us`. Matched exactly,
    /// ignoring case.
    pub name: String,
    /// What to call the app in the prompt, e.g. `Zoom`.
    pub label: String,
    /// The app is open all day (Slack, Discord), so it being open says nothing
    /// on its own. It only prompts when a call signal — a calendar event or
    /// audio activity — happened recently too. See [`crate::Detector`].
    pub needs_call_signal: bool,
}

/// Every meeting app meet-ai looks for.
pub fn meeting_processes() -> &'static [MeetingProcess] {
    static LIST: OnceLock<Vec<MeetingProcess>> = OnceLock::new();
    LIST.get_or_init(|| {
        parse(PROCESSES_JSON).unwrap_or_else(|error| {
            // The file is compiled in and parsed by a unit test, so this is a
            // build mistake. Detecting nothing beats taking the app down.
            tracing::error!(%error, "processes.json did not parse; process detection is off");
            Vec::new()
        })
    })
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

/// The name to show for a process name: its label if it is a known meeting
/// app, else the name itself.
pub fn label(process_name: &str) -> &str {
    find(process_name).map_or(process_name, |known| known.label.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_list_parses_and_has_the_spec_2_3_apps() {
        let list = parse(PROCESSES_JSON).expect("processes.json must parse");
        let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["zoom.us", "Microsoft Teams", "Webex", "Slack", "Discord"]
        );
        assert_eq!(meeting_processes().len(), 5);
    }

    #[test]
    fn only_slack_and_discord_need_a_call_signal() {
        let needing: Vec<&str> = meeting_processes()
            .iter()
            .filter(|p| p.needs_call_signal)
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(needing, ["Slack", "Discord"]);
    }

    #[test]
    fn matching_is_exact_but_ignores_case() {
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
        assert_eq!(label("zoom.us"), "Zoom");
        assert_eq!(label("SomethingElse"), "SomethingElse");
    }

    #[test]
    fn a_new_platform_is_a_data_edit() {
        let list =
            parse(r#"[{ "name": "Zoom.exe", "label": "Zoom", "needs_call_signal": false }]"#)
                .expect("parses");
        assert_eq!(
            find_in(&list, "zoom.exe").map(|p| p.label.as_str()),
            Some("Zoom")
        );
    }
}
