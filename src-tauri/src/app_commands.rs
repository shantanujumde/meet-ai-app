//! Every app command, by name, for the build script's app manifest (TUR-158).
//!
//! `build.rs` hands this list to `tauri_build::AppManifest::commands`, which
//! makes Tauri check app commands against the capability files too: a window
//! can call a command only if its capability grants `allow-<command>`. Without
//! the manifest every window could call every command. Included by `build.rs`
//! through `#[path]`, so this file holds only the list (and its tests).
//!
//! Adding a command: list it in `bindings.rs` and here, and grant
//! `allow-<command>` (kebab case) to the window that calls it in
//! `capabilities/`. The tests below fail until all three agree.

/// The commands `bindings.rs` registers, in the same order.
pub const APP_COMMANDS: &[&str] = &[
    "list_meetings",
    "read_meeting",
    "save_notes",
    "rename_meeting",
    "change_meetings_folder",
    "reveal_meeting",
    "delete_meeting",
    "measure_permission",
    "permission_quick",
    "open_privacy_settings",
    "onboarding_state",
    "complete_onboarding",
    "reset_onboarding",
    "engine_environment",
    "engine_selection",
    "model_catalogue",
    "download_model",
    "delete_model",
    "engine_choices",
    "model_credits",
    "set_transcription",
    "set_spoken_language",
    "recording_status",
    "toggle_recording",
    "stop_recording",
    "pause_recording",
    "resume_recording",
    "live_transcript",
    "list_tickets",
    "create_ticket",
    "start_work_prompt",
    "wrap_up_prompt",
    "copy_prompt_fallback",
    "agent_choice",
    "detect_agents",
    "save_agent_choice",
    "test_agent",
    "notes_auto_run",
    "save_notes_auto_run",
    "search",
    "notes_run_status",
    "start_notes_run",
    "cancel_notes_run",
    "meeting_notes",
    "sync_task",
    "cancel_sync",
    "dismiss_unsaved_sync",
    "meeting_tasks",
    "open_synced_issue",
    "approve_task",
    "approve_all_tasks",
    "discard_task",
    "tracker_settings",
    "set_tracker",
    "tracker_servers",
    "ticket_sync_states",
    "retry_ticket_sync",
    "send_test_ticket",
    "set_meeting_notes",
    "todays_meetings",
    "calendar_refresh_minutes",
    "calendar_accounts",
    "calendar_sources",
    "set_calendar_app",
    "calendar_connect",
    "calendar_disconnect",
    "calendar_cancel_sign_in",
    "meeting_brief",
    "audio_retention_days",
    "confirm_quit",
    "app_settings",
    "set_show_in_dock_when_closed",
    "open_logs_folder",
    "menu_bar_countdown",
    "set_menu_bar_countdown",
    "builtin_mic_with_bluetooth",
    "set_builtin_mic_with_bluetooth",
    "notification_settings",
    "set_notification_settings",
    "os_notifications_blocked",
    "open_notification_settings",
    "send_test_reminder",
    "never_detect_apps",
    "set_never_detect_apps",
    "join_reminded_meeting",
    "record_reminded_meeting",
    "prompt_popup_current",
    "answer_prompt_popup",
    "record_shortcut_available",
    "start_at_login",
    "set_start_at_login",
    "appearance_settings",
    "set_appearance",
    "headphone_warning",
    "meetings_watch_problem",
    "show_recording_overlay",
    "set_show_recording_overlay",
    "overlay_show_main",
    "config_problem",
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::APP_COMMANDS;

    /// What the "Record this meeting?" card calls: its card, its answer, and
    /// the saved Light / Dark choice (`watchAppearance` in `main.tsx`).
    const PROMPT_WINDOW: &[&str] = &[
        "prompt_popup_current",
        "answer_prompt_popup",
        "appearance_settings",
    ];

    /// What the recording overlay calls (`src/ui/Overlay.tsx`).
    const OVERLAY_WINDOW: &[&str] = &[
        "recording_status",
        "pause_recording",
        "resume_recording",
        "stop_recording",
        "live_transcript",
        "overlay_show_main",
        "appearance_settings",
    ];

    /// Only the small windows call these; the main window is not granted them.
    const NOT_FOR_MAIN: &[&str] = &[
        "prompt_popup_current",
        "answer_prompt_popup",
        "overlay_show_main",
    ];

    fn manifest_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// The app-command permissions (no `plugin:` prefix) a capability grants,
    /// turned back into command names.
    fn granted_commands(capability: &str) -> BTreeSet<String> {
        let path = manifest_dir().join("capabilities").join(capability);
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry.as_str())
            .filter(|id| !id.contains(':'))
            .map(|id| {
                let command = id.strip_prefix("allow-").unwrap_or_else(|| {
                    panic!("{capability}: {id:?} is an app permission but not allow-<command>")
                });
                command.replace('-', "_")
            })
            .collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// Every command name `bindings.ts` invokes.
    fn invoked_in_bindings(bindings: &Path) -> BTreeSet<String> {
        let text = std::fs::read_to_string(bindings).unwrap();
        text.split("__TAURI_INVOKE")
            .skip(1)
            .filter(|call| call.starts_with(['(', '<']))
            .filter_map(|call| {
                let name = &call[call.find("(\"")? + 2..];
                Some(name[..name.find('"')?].to_owned())
            })
            .collect()
    }

    #[test]
    fn the_list_matches_the_registered_commands() {
        let listed = set(APP_COMMANDS);
        assert_eq!(
            listed.len(),
            APP_COMMANDS.len(),
            "a command is listed twice"
        );
        // A fresh export, not the committed file: CI deletes that one for
        // `export_bindings` to write again while this test runs.
        let scratch = tempfile::tempdir().unwrap();
        let bindings = scratch.path().join("bindings.ts");
        crate::bindings::builder()
            .export(
                specta_typescript::Typescript::default()
                    .layout(specta_typescript::Layout::ModulePrefixedName),
                &bindings,
            )
            .unwrap();
        let registered = invoked_in_bindings(&bindings);
        let unlisted: Vec<_> = registered.difference(&listed).collect();
        let unregistered: Vec<_> = listed.difference(&registered).collect();
        assert!(
            unlisted.is_empty() && unregistered.is_empty(),
            "APP_COMMANDS and bindings.rs disagree: add {unlisted:?} to APP_COMMANDS \
             (and grant it in capabilities/), drop {unregistered:?}"
        );
    }

    #[test]
    fn the_prompt_card_can_call_only_its_own_commands() {
        assert_eq!(granted_commands("prompt.json"), set(PROMPT_WINDOW));
    }

    #[test]
    fn the_overlay_can_call_only_its_own_commands() {
        assert_eq!(granted_commands("overlay.json"), set(OVERLAY_WINDOW));
    }

    #[test]
    fn the_main_window_gets_every_command_but_the_small_windows_own() {
        let expected: BTreeSet<String> = set(APP_COMMANDS)
            .into_iter()
            .filter(|command| !NOT_FOR_MAIN.contains(&command.as_str()))
            .collect();
        assert_eq!(granted_commands("default.json"), expected);
    }

    /// The main window's opener scope (`opener:allow-open-url` in
    /// `default.json`), as its `url` patterns.
    fn opener_scope() -> Vec<String> {
        let path = manifest_dir().join("capabilities").join("default.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["identifier"] == "opener:allow-open-url")
            .flat_map(|entry| entry["allow"].as_array().unwrap().iter())
            .map(|allow| allow["url"].as_str().unwrap().to_owned())
            .collect()
    }

    /// `*` stands for any run of characters, the rest must match exactly.
    fn glob_matches(pattern: &str, text: &str) -> bool {
        let mut parts = pattern.split('*');
        let first = parts.next().unwrap_or_default();
        let Some(mut rest) = text.strip_prefix(first) else {
            return false;
        };
        let parts: Vec<&str> = parts.collect();
        let Some((last, middle)) = parts.split_last() else {
            return rest.is_empty();
        };
        for part in middle {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
        rest.ends_with(last)
    }

    #[test]
    fn the_glob_check_matches_like_the_scope() {
        assert!(glob_matches("https://a.co/x/*", "https://a.co/x/y"));
        assert!(!glob_matches("https://a.co/x/*", "https://a.co/z/y"));
        assert!(glob_matches("https://a.co/x", "https://a.co/x"));
        assert!(!glob_matches("https://a.co/x", "https://a.co/xy"));
    }

    /// Settings, About opens every model credit through the opener, so a
    /// credit outside the main window's scope would be refused without a word.
    #[test]
    fn every_model_credit_link_is_in_the_opener_scope() {
        let scope = opener_scope();
        assert!(!scope.is_empty(), "default.json has no opener scope");
        for credit in crate::engine::credits() {
            assert!(
                scope
                    .iter()
                    .any(|pattern| glob_matches(pattern, credit.url)),
                "{} is outside the opener scope {scope:?} in capabilities/default.json",
                credit.url
            );
        }
    }

    #[test]
    fn every_window_name_in_the_tests_is_a_real_command() {
        let all = set(APP_COMMANDS);
        for command in PROMPT_WINDOW
            .iter()
            .chain(OVERLAY_WINDOW)
            .chain(NOT_FOR_MAIN)
        {
            assert!(all.contains(*command), "{command} is not an app command");
        }
    }
}
