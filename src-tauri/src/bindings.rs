//! The command list, shared by the running app and the TypeScript generator.
//!
//! `src/ipc/bindings.ts` is generated from [`builder`] by the `export_bindings`
//! test below (`just bindings`). It never needs the app to run, so it works in
//! CI. Adding a command: put `#[specta::specta]` under its `#[tauri::command]`
//! and list it here.
//! Then list it in `app_commands.rs` and grant `allow-<command>` to the window
//! that calls it in `capabilities/`, or Tauri refuses the call (TUR-158).

use tauri_specta::{Builder, collect_commands};

use crate::agent_run;
use crate::agent_setup;
use crate::brief;
use crate::events::AGENT_RUN_STATUS_EVENT;
use crate::events::DETECTION_PROMPT_EVENT;
use crate::events::HEADPHONE_WARNING_EVENT;
use crate::events::HOOK_FAILED_EVENT;
use crate::events::MEETINGS_WATCH_PROBLEM_EVENT;
use crate::events::NAVIGATE_EVENT;
use crate::events::PROMPT_POPUP_EVENT;
use crate::events::QUIT_CONFIRM_EVENT;
use crate::events::TICKET_SYNC_EVENT;
use crate::events::{
    MEETINGS_CHANGED_EVENT, MODEL_PROGRESS_EVENT, PERMISSION_STATUS_EVENT, RECORDING_STATE_EVENT,
    TRANSCRIPT_STATUS_EVENT, TRANSCRIPT_UPDATE_EVENT,
};
use crate::sync::{self, tracker};
use crate::{commands, search};

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::list_meetings,
            commands::read_meeting,
            commands::save_notes,
            commands::rename_meeting,
            commands::change_meetings_folder,
            commands::reveal_meeting,
            crate::meetings::delete::delete_meeting,
            commands::measure_permission,
            commands::permission_quick,
            commands::open_privacy_settings,
            commands::onboarding_state,
            commands::complete_onboarding,
            commands::reset_onboarding,
            commands::engine_environment,
            commands::engine_selection,
            commands::model_catalogue,
            commands::download_model,
            commands::delete_model,
            commands::engine_choices,
            commands::model_credits,
            commands::set_transcription,
            commands::set_spoken_language,
            commands::recording_status,
            commands::toggle_recording,
            commands::stop_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::live_transcript,
            commands::list_tickets,
            commands::create_ticket,
            commands::start_work_prompt,
            commands::wrap_up_prompt,
            commands::copy_prompt_fallback,
            agent_setup::agent_choice,
            agent_setup::detect_agents,
            agent_setup::save_agent_choice,
            agent_setup::test_agent,
            agent_setup::cancel_agent_test,
            agent_setup::notes_auto_run,
            agent_setup::save_notes_auto_run,
            search::search,
            agent_run::notes_run_status,
            agent_run::start_notes_run,
            agent_run::cancel_notes_run,
            agent_run::meeting_notes,
            sync::sync_task,
            sync::cancel_sync,
            sync::dismiss_unsaved_sync,
            sync::meeting_tasks,
            sync::open_synced_issue,
            crate::tickets::approve_task,
            crate::tickets::approve_all_tasks,
            crate::tickets::discard_task,
            tracker::tracker_settings,
            tracker::set_tracker,
            tracker::tracker_servers,
            sync::auto::ticket_sync_states,
            sync::auto::retry_ticket_sync,
            sync::check::send_test_ticket,
            agent_run::set_meeting_notes,
            crate::calendar::todays_meetings,
            crate::calendar::calendar_refresh_minutes,
            crate::calendar::signin::calendar_accounts,
            crate::calendar::sources::calendar_sources,
            crate::calendar::sources::set_calendar_app,
            crate::calendar::sources::calendar_connect,
            crate::calendar::sources::calendar_disconnect,
            crate::calendar::cancel::calendar_cancel_sign_in,
            brief::meeting_brief,
            crate::retention::audio_retention_days,
            crate::lifecycle::confirm_quit,
            crate::lifecycle::app_settings,
            crate::lifecycle::set_show_in_dock_when_closed,
            crate::logs::open_logs_folder,
            crate::lifecycle::menu_bar_countdown,
            crate::lifecycle::set_menu_bar_countdown,
            crate::mic_setting::builtin_mic_with_bluetooth,
            crate::mic_setting::set_builtin_mic_with_bluetooth,
            crate::detection::settings::notification_settings,
            crate::detection::settings::set_notification_settings,
            crate::detection::settings::os_notifications_blocked,
            crate::detection::settings::open_notification_settings,
            crate::detection::settings::send_test_reminder,
            crate::detection::settings::never_detect_apps,
            crate::detection::settings::set_never_detect_apps,
            crate::detection::actions::join_reminded_meeting,
            crate::detection::actions::record_reminded_meeting,
            crate::detection::popup::prompt_popup_current,
            crate::detection::popup::answer_prompt_popup,
            crate::shortcut::record_shortcut_available,
            crate::autostart::start_at_login,
            crate::autostart::set_start_at_login,
            crate::appearance::appearance_settings,
            crate::appearance::set_appearance,
            crate::headphone_warning::headphone_warning,
            crate::watch::meetings_watch_problem,
            crate::overlay::show_recording_overlay,
            crate::overlay::set_show_recording_overlay,
            crate::overlay::overlay_show_main,
            crate::config_problem::config_problem,
        ])
        .constant("RECORDING_STATE_EVENT", RECORDING_STATE_EVENT)
        .constant("MODEL_PROGRESS_EVENT", MODEL_PROGRESS_EVENT)
        .constant("PERMISSION_STATUS_EVENT", PERMISSION_STATUS_EVENT)
        .constant("TRANSCRIPT_UPDATE_EVENT", TRANSCRIPT_UPDATE_EVENT)
        .constant("TRANSCRIPT_STATUS_EVENT", TRANSCRIPT_STATUS_EVENT)
        .constant("MEETINGS_CHANGED_EVENT", MEETINGS_CHANGED_EVENT)
        .constant("AGENT_RUN_STATUS_EVENT", AGENT_RUN_STATUS_EVENT)
        .constant("DETECTION_PROMPT_EVENT", DETECTION_PROMPT_EVENT)
        .constant("QUIT_CONFIRM_EVENT", QUIT_CONFIRM_EVENT)
        .constant("NAVIGATE_EVENT", NAVIGATE_EVENT)
        .constant("PROMPT_POPUP_EVENT", PROMPT_POPUP_EVENT)
        .constant("HOOK_FAILED_EVENT", HOOK_FAILED_EVENT)
        .constant("HEADPHONE_WARNING_EVENT", HEADPHONE_WARNING_EVENT)
        .constant("MEETINGS_WATCH_PROBLEM_EVENT", MEETINGS_WATCH_PROBLEM_EVENT)
        .constant("TICKET_SYNC_EVENT", TICKET_SYNC_EVENT)
        .constant("RECORD_SHORTCUT_MAC", crate::shortcut::RECORD_SHORTCUT_MAC)
        .constant(
            "RECORD_SHORTCUT_OTHER",
            crate::shortcut::RECORD_SHORTCUT_OTHER,
        )
        // TUR-155: the one range `detection.min_attendees` takes.
        .constant("MIN_ATTENDEES", crate::config::MIN_ATTENDEES)
        .constant("MAX_ATTENDEES", crate::config::MAX_ATTENDEES)
        .typ::<crate::detection::notify::Prompt>()
        .typ::<crate::lifecycle::NavigateTo>()
        .typ::<crate::hooks::app::HookFailed>()
        .typ::<crate::watch::WatchProblem>()
}

#[cfg(test)]
mod tests {
    use specta_typescript::{Layout, Typescript};

    /// Writes `src/ipc/bindings.ts`. Run by `just bindings`; CI then fails if the
    /// committed file differs.
    #[test]
    fn export_bindings() {
        // `BINDINGS_OUT` lets the quality gate (rule R7) generate into a temp
        // copy and compare, without touching the working tree.
        let path = std::env::var("BINDINGS_OUT").unwrap_or_else(|_| {
            concat!(env!("CARGO_MANIFEST_DIR"), "/../src/ipc/bindings.ts").to_owned()
        });
        // Flat file, module path in each type name: two modules have a `State`
        // and a `Status`, and the namespace layout cannot compile under this
        // repo's `verbatimModuleSyntax`.
        super::builder()
            .export(
                Typescript::default().layout(Layout::ModulePrefixedName),
                &path,
            )
            .expect("could not write src/ipc/bindings.ts");
    }
}
