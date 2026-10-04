//! The "Record this meeting?" popup window (TUR-59).
//!
//! The notification plugin's action buttons work on mobile only, and on
//! Windows a toast from an app without an installed AppUserModelID is signed
//! "PowerShell". So on Windows and Linux a prompt shows in a small window of
//! our own: always on top, no taskbar entry, no focus taken, top-right of the
//! primary monitor's work area ([`window`]). It holds the reason and
//! **Record** / **Dismiss** (and **Join and record** / **Join** for a
//! reminder with a meeting link, as TUR-78's banner), and hides itself after
//! [`AUTO_HIDE`].
//!
//! Whether a platform uses it is decided in one place, [`platform::USE_POPUP`]:
//! macOS keeps its notification and in-window banner unchanged.
//!
//! One popup at a time ([`Slot`]): a new prompt replaces the one on screen,
//! and every prompt has its own id, so a click or a timer for an older one does
//! nothing. Record goes through the same start paths as the banner and the
//! menu bar ([`super::actions::record_reminded_meeting`],
//! `folder_move::start_recording`); nothing starts without a click (L15).

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, Manager as _};

use super::notify::Prompt;
use crate::error::UiError;
use crate::events::PROMPT_POPUP_EVENT;
use crate::lock::lock_or_recover;

mod platform;
mod window;

/// How long a popup stays up unanswered. Unanswered is "not now": nothing
/// records.
pub const AUTO_HIDE: Duration = Duration::from_secs(20);

/// What the popup window shows, on [`PROMPT_POPUP_EVENT`] and from
/// [`prompt_popup_current`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PopupPrompt {
    /// This prompt's id; an answer names it, so a stale click is ignored.
    /// A JS number: ids count up from 1 and never get near 2^53.
    pub id: u32,
    pub prompt: Prompt,
}

/// A button in the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PopupAnswer {
    Record,
    JoinAndRecord,
    Join,
    Dismiss,
}

/// What an answer makes meet-ai do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Close the popup; record nothing.
    Nothing,
    /// Start a recording: named after the reminded event when there is one,
    /// joining its meeting first when `join`.
    Start {
        event_id: Option<String>,
        join: bool,
    },
    /// Open the reminded meeting's link; the popup stays up.
    Join { event_id: String },
}

/// The one prompt on screen, if any.
#[derive(Debug, Default)]
pub struct Slot {
    last_id: u32,
    current: Option<PopupPrompt>,
}

impl Slot {
    /// Show `prompt`, replacing the one on screen. An `update_only` prompt
    /// (the same call noticed a second way) only replaces: with nothing on
    /// screen it opens nothing, and `None` comes back.
    pub fn show(&mut self, prompt: Prompt) -> Option<PopupPrompt> {
        if prompt.update_only && self.current.is_none() {
            return None;
        }
        self.last_id = self.last_id.wrapping_add(1).max(1);
        let shown = PopupPrompt {
            id: self.last_id,
            prompt,
        };
        self.current = Some(shown.clone());
        Some(shown)
    }

    /// The prompt on screen.
    pub fn current(&self) -> Option<PopupPrompt> {
        self.current.clone()
    }

    /// [`AUTO_HIDE`] passed for prompt `id`: close it if it is still the one
    /// on screen. `true` when the window should hide.
    pub fn expire(&mut self, id: u32) -> bool {
        self.close_if(id)
    }

    /// Close prompt `id` if it is on screen, for any reason (a recording
    /// started elsewhere, an answer).
    pub fn close_if(&mut self, id: u32) -> bool {
        if self.current.as_ref().is_some_and(|shown| shown.id == id) {
            self.current = None;
            true
        } else {
            false
        }
    }

    /// The user pressed `answer` on prompt `id`. `None` when that prompt is
    /// no longer on screen (replaced, timed out, already answered): a second
    /// click on Record starts nothing. Every answer but Join closes it.
    pub fn answer(&mut self, id: u32, answer: PopupAnswer) -> Option<Action> {
        let shown = self.current.as_ref().filter(|shown| shown.id == id)?;
        let action = action_for(&shown.prompt, answer);
        if answer != PopupAnswer::Join || matches!(action, Action::Nothing) {
            self.current = None;
        }
        Some(action)
    }
}

/// What `answer` on `prompt` does. A test reminder never records or joins.
pub fn action_for(prompt: &Prompt, answer: PopupAnswer) -> Action {
    if prompt.test {
        return Action::Nothing;
    }
    let event_id = prompt.event_id.clone();
    match answer {
        PopupAnswer::Dismiss => Action::Nothing,
        PopupAnswer::Record => Action::Start {
            event_id,
            join: false,
        },
        PopupAnswer::JoinAndRecord if prompt.can_join => Action::Start {
            event_id,
            join: true,
        },
        PopupAnswer::Join => match event_id {
            Some(event_id) if prompt.can_join => Action::Join { event_id },
            _ => Action::Nothing,
        },
        PopupAnswer::JoinAndRecord => Action::Nothing,
    }
}

/// The popup's state, managed by the app.
#[derive(Debug, Default)]
pub struct PromptPopup(Mutex<Slot>);

impl PromptPopup {
    fn with<T>(&self, f: impl FnOnce(&mut Slot) -> T) -> T {
        f(&mut lock_or_recover(&self.0))
    }
}

/// Does this platform show prompts in the popup? The one switch.
pub fn enabled() -> bool {
    platform::USE_POPUP
}

/// Show `prompt` in the popup. `true` when the popup took it (shown, or an
/// `update_only` prompt with nothing on screen to update); `false` when the
/// window could not be made, so the caller falls back to a notification.
pub fn show(app: &AppHandle, prompt: &Prompt) -> bool {
    let Some(state) = app.try_state::<PromptPopup>() else {
        tracing::warn!("the prompt popup is not set up; falling back to a notification");
        return false;
    };
    let Some(shown) = state.with(|slot| slot.show(prompt.clone())) else {
        return true;
    };
    if let Err(error) = window::show(app) {
        tracing::warn!(%error, "could not show the prompt popup; falling back to a notification");
        state.with(|slot| slot.close_if(shown.id));
        return false;
    }
    if let Err(error) = app.emit_to(window::LABEL, PROMPT_POPUP_EVENT, &shown) {
        tracing::warn!(%error, "could not send the prompt to its popup");
    }
    let app = app.clone();
    let id = shown.id;
    std::thread::spawn(move || {
        std::thread::sleep(AUTO_HIDE);
        let expired = app
            .try_state::<PromptPopup>()
            .is_some_and(|state| state.with(|slot| slot.expire(id)));
        if expired {
            tracing::debug!("the prompt popup timed out; nothing recorded");
            window::hide(&app);
        }
    });
    true
}

/// The prompt the popup should show, for a window that just loaded.
#[tauri::command]
#[specta::specta]
pub async fn prompt_popup_current(app: AppHandle) -> Option<PopupPrompt> {
    app.try_state::<PromptPopup>()
        .and_then(|state| state.with(|slot| slot.current()))
}

/// A popup button was pressed for prompt `id`.
#[tauri::command]
#[specta::specta]
pub async fn answer_prompt_popup(
    app: AppHandle,
    id: u32,
    answer: PopupAnswer,
) -> Result<(), UiError> {
    let action = app
        .try_state::<PromptPopup>()
        .and_then(|state| state.with(|slot| slot.answer(id, answer)));
    let Some(action) = action else {
        // Replaced or timed out; the window may still be up from a race.
        if app
            .try_state::<PromptPopup>()
            .is_none_or(|state| state.with(|slot| slot.current().is_none()))
        {
            window::hide(&app);
        }
        return Ok(());
    };
    if !matches!(action, Action::Join { .. }) {
        window::hide(&app);
    }
    run(&app, action).await
}

async fn run(app: &AppHandle, action: Action) -> Result<(), UiError> {
    match action {
        Action::Nothing => Ok(()),
        Action::Join { event_id } => {
            super::actions::join_reminded_meeting(app.clone(), event_id).await
        }
        Action::Start {
            event_id: Some(event_id),
            join,
        } => super::actions::record_reminded_meeting(app.clone(), event_id, join)
            .await
            .map(drop),
        Action::Start {
            event_id: None,
            join: _,
        } => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || crate::folder_move::start_recording(&app))
                .await
                .map_err(|error| UiError::app("record-failed", error.to_string()))?
                .map(drop)
        }
    }
}

#[cfg(test)]
mod tests;
