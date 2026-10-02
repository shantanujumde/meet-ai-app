//! The command list, shared by the running app and the TypeScript generator.
//!
//! `src/ipc/bindings.ts` is generated from [`builder`] by the `export_bindings`
//! test below (`just bindings`). It never needs the app to run, so it works in
//! CI. Adding a command: put `#[specta::specta]` under its `#[tauri::command]`
//! and list it here.

use tauri_specta::{Builder, collect_commands};

use crate::events::{
    MEETINGS_CHANGED_EVENT, MODEL_PROGRESS_EVENT, PERMISSION_STATUS_EVENT, RECORDING_STATE_EVENT,
    TRANSCRIPT_STATUS_EVENT, TRANSCRIPT_UPDATE_EVENT,
};
use crate::{commands, search};

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::list_meetings,
            commands::read_meeting,
            commands::save_notes,
            commands::change_meetings_folder,
            commands::reveal_meeting,
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
            commands::recording_status,
            commands::toggle_recording,
            commands::stop_recording,
            commands::live_transcript,
            commands::list_tickets,
            commands::create_ticket,
            commands::start_work_prompt,
            commands::wrap_up_prompt,
            commands::copy_prompt_fallback,
            search::search,
        ])
        .constant("RECORDING_STATE_EVENT", RECORDING_STATE_EVENT)
        .constant("MODEL_PROGRESS_EVENT", MODEL_PROGRESS_EVENT)
        .constant("PERMISSION_STATUS_EVENT", PERMISSION_STATUS_EVENT)
        .constant("TRANSCRIPT_UPDATE_EVENT", TRANSCRIPT_UPDATE_EVENT)
        .constant("TRANSCRIPT_STATUS_EVENT", TRANSCRIPT_STATUS_EVENT)
        .constant("MEETINGS_CHANGED_EVENT", MEETINGS_CHANGED_EVENT)
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
