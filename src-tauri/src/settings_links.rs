//! Where each "open settings" button lands, per OS (TUR-51).
//!
//! A pure table, keyed on the OS name (`std::env::consts::OS`) and, on Linux,
//! the desktop (`XDG_CURRENT_DESKTOP`), so every OS's answer is unit-tested
//! on the Mac that builds this. `commands::open_privacy_settings` tries the
//! candidates in order and stops at the first that opens.
//!
//! * macOS: the Privacy & Security anchor, then the pane root (SPEC §8.1).
//! * Windows: the `ms-settings:` page, through the opener, then through
//!   `cmd /C start "" <uri>` the way Handy does it.
//! * Linux: the desktop's sound settings (GNOME and its relatives, KDE), or
//!   nothing: Linux has no audio permission to grant, so there is no
//!   privacy page to find.

use crate::permission::{PRIVACY_ROOT_URL, Pane};

/// One way to open a settings page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Open through the opener plugin.
    Url(&'static str),
    /// Spawn a program with arguments.
    Command(&'static str, Vec<&'static str>),
}

/// The Windows settings page for a pane.
fn windows_uri(pane: Pane) -> &'static str {
    match pane {
        Pane::Microphone => "ms-settings:privacy-microphone",
        Pane::AudioCapture => "ms-settings:sound",
        Pane::Calendars => "ms-settings:privacy-calendar",
    }
}

/// The sound-settings program for a Linux desktop, from
/// `XDG_CURRENT_DESKTOP` (a `:`-separated list, such as `ubuntu:GNOME`).
fn linux_sound_settings(desktop: &str) -> Option<Target> {
    let names: Vec<String> = desktop
        .split(':')
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();
    let has = |wanted: &[&str]| names.iter().any(|name| wanted.contains(&name.as_str()));
    if has(&["kde", "plasma"]) {
        Some(Target::Command("systemsettings", vec!["kcm_pulseaudio"]))
    } else if has(&["gnome", "unity", "budgie", "pop", "ubuntu"]) {
        Some(Target::Command("gnome-control-center", vec!["sound"]))
    } else {
        None
    }
}

/// Every way to open `pane` on `os`, best first. Empty when this OS has no
/// such page (Linux calendars, an unknown desktop).
pub fn targets(os: &str, desktop: Option<&str>, pane: Pane) -> Vec<Target> {
    match os {
        "macos" => vec![Target::Url(pane.url()), Target::Url(PRIVACY_ROOT_URL)],
        "windows" => {
            let uri = windows_uri(pane);
            vec![
                Target::Url(uri),
                // Adapted from github.com/cjpais/Handy/src-tauri/src/commands/audio.rs @ 73ab851c2b6242283759a4c101b60f0ece132f08 (MIT)
                Target::Command("cmd", vec!["/C", "start", "", uri]),
            ]
        }
        "linux" if pane != Pane::Calendars => {
            desktop.and_then(linux_sound_settings).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// The candidates for this machine.
pub fn targets_here(pane: Pane) -> Vec<Target> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    targets(std::env::consts::OS, desktop.as_deref(), pane)
}

/// The OS settings app's name, for sentences ("… in System Settings").
pub fn settings_name(os: &str) -> &'static str {
    match os {
        "macos" => "System Settings",
        "windows" => "Windows Settings",
        _ => "your system settings",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_keeps_the_anchor_then_the_pane_root() {
        assert_eq!(
            targets("macos", None, Pane::Microphone),
            vec![
                Target::Url(Pane::Microphone.url()),
                Target::Url(PRIVACY_ROOT_URL)
            ]
        );
    }

    #[test]
    fn windows_opens_the_ms_settings_pages() {
        let mic = targets("windows", None, Pane::Microphone);
        assert_eq!(mic[0], Target::Url("ms-settings:privacy-microphone"));
        assert_eq!(
            mic[1],
            Target::Command(
                "cmd",
                vec!["/C", "start", "", "ms-settings:privacy-microphone"]
            )
        );
        assert_eq!(
            targets("windows", None, Pane::AudioCapture)[0],
            Target::Url("ms-settings:sound")
        );
    }

    #[test]
    fn linux_opens_the_desktops_sound_settings_or_nothing() {
        let gnome = Target::Command("gnome-control-center", vec!["sound"]);
        let kde = Target::Command("systemsettings", vec!["kcm_pulseaudio"]);
        assert_eq!(
            targets("linux", Some("ubuntu:GNOME"), Pane::Microphone),
            vec![gnome.clone()]
        );
        assert_eq!(
            targets("linux", Some("GNOME"), Pane::AudioCapture),
            vec![gnome]
        );
        assert_eq!(targets("linux", Some("KDE"), Pane::Microphone), vec![kde]);
        assert!(targets("linux", Some("XFCE"), Pane::Microphone).is_empty());
        assert!(targets("linux", None, Pane::Microphone).is_empty());
        assert!(targets("linux", Some("GNOME"), Pane::Calendars).is_empty());
    }

    #[test]
    fn every_url_scheme_is_in_the_opener_scope() {
        // `capabilities/default.json` allows these schemes for the opener.
        let capability = include_str!("../capabilities/default.json");
        for os in ["macos", "windows"] {
            for pane in [Pane::Microphone, Pane::AudioCapture, Pane::Calendars] {
                for target in targets(os, None, pane) {
                    if let Target::Url(url) = target {
                        let (scheme, _) = url.split_once(':').unwrap_or((url, ""));
                        assert!(
                            capability.contains(&format!("\"{scheme}:*\"")),
                            "{scheme}: is missing from the opener scope"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_settings_app_is_named_per_os() {
        assert_eq!(settings_name("macos"), "System Settings");
        assert_eq!(settings_name("windows"), "Windows Settings");
        assert_eq!(settings_name("linux"), "your system settings");
    }
}
