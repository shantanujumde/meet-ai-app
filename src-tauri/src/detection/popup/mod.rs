//! The prompt card window (TUR-59, restyled in TUR-108 and TUR-147).
//!
//! The notification plugin's action buttons work on mobile only, and on
//! Windows a toast from an app without an installed AppUserModelID is signed
//! "PowerShell". So a prompt shows in a small window of our own: always on
//! top, no taskbar entry, no focus taken, transparent around a rounded card,
//! top-right of the primary monitor's work area (under the macOS menu bar;
//! [`window`]). On macOS it is a non-activating panel that shows on every
//! Space and over full-screen apps ([`platform`]).
//!
//! The window holds one of three cards ([`Card`], [`window::Layout`]):
//!
//! * a calendar reminder (TUR-108): the meeting's title and time, and one
//!   split button: **Join Meet & record** with a meeting link, **Record**
//!   otherwise, and a chevron menu with Join only, Record only, Open brief
//!   and Dismiss;
//! * a detection prompt (TUR-147): our icon, one line naming the app ("Zoom
//!   call"), a big **Record**, a quiet **Not now**, and a "⋯" menu with
//!   **Never for Zoom**;
//! * a countdown (TUR-147, [`countdown`]): "Zoom call ended", a ring counting
//!   down, **Stop now** and **Keep recording**. TUR-144 and TUR-145 show it.
//!
//! A prompt hides itself after [`AUTO_HIDE`]; a countdown when it runs out.
//! Every prompt uses the card on every OS ([`uses_popup`]); the OS
//! notification is the fallback when the window cannot be shown.
//!
//! One card at a time ([`Slot`]): a new one replaces the one on screen, and
//! every card has its own id, so a click or a timer for an older one does
//! nothing. Record goes through the same start paths as the banner and the
//! menu bar ([`super::actions::record_reminded_meeting`],
//! `folder_move::start_recording`); nothing starts without a click (L15).

use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::Duration;

use detect::Signal;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter as _, Manager as _};

use super::notify::Prompt;
use crate::error::UiError;
use crate::events::PROMPT_POPUP_EVENT;
use crate::lifecycle::NavigateTo;
use crate::lock::lock_or_recover;

pub mod countdown;
mod platform;
mod window;

pub use countdown::CountdownEnd;
// TUR-146: the recording overlay is the same kind of panel, shown the same way.
pub(crate) use platform::{make_panel, show as show_floating};

/// How long a prompt stays up unanswered. Unanswered is "not now": nothing
/// records.
pub const AUTO_HIDE: Duration = Duration::from_secs(20);

/// How long the window stays up after its card closes, so the card can fade
/// out first (the card's own transition is shorter; with Reduce Motion it
/// does not fade at all).
pub const FADE_OUT: Duration = Duration::from_millis(200);

/// What the popup window shows, on [`PROMPT_POPUP_EVENT`] and from
/// [`prompt_popup_current`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PopupPrompt {
    /// This card's id; an answer names it, so a stale click is ignored.
    /// A JS number: ids count up from 1 and never get near 2^53.
    pub id: u32,
    pub card: Card,
    /// When the card closes on its own, in Unix milliseconds: the end of a
    /// prompt's [`AUTO_HIDE`] or of a countdown. The window counts down to
    /// it and fades out there. A JS number, far below 2^53.
    #[specta(type = specta_typescript::Number)]
    pub closes_at_ms: i64,
}

/// What the card asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Card {
    /// "Record this meeting?": a reminder's card for a calendar signal, the
    /// narrow detection card for the rest.
    Prompt { prompt: Prompt },
    /// "Zoom call ended": stopping in `seconds` unless the user keeps
    /// recording ([`countdown`]).
    Countdown { line: String, seconds: u32 },
}

impl From<Prompt> for Card {
    fn from(prompt: Prompt) -> Self {
        Self::Prompt { prompt }
    }
}

/// A button in the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PopupAnswer {
    Record,
    JoinAndRecord,
    Join,
    // The chevron menu's **Open brief** (TUR-108).
    OpenBrief,
    /// **Dismiss**, and the detection card's **Not now**.
    Dismiss,
    /// The "⋯" menu's **Never for <App>** (TUR-147). The app is the one the
    /// prompt on screen names ([`Prompt::app`]), never one the window sends.
    NeverFor,
    /// The countdown's **Stop now** (TUR-147).
    StopNow,
    /// The countdown's **Keep recording** (TUR-147).
    KeepRecording,
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
    /// Bring the main window forward on the brief for the meeting called
    /// `title`; record nothing.
    OpenBrief { title: String },
    /// Never ask about `app` again (TUR-143 keeps the list); record nothing.
    NeverFor { app: String },
}

/// The one card on screen, if any.
#[derive(Debug, Default)]
pub struct Slot {
    last_id: u32,
    current: Option<PopupPrompt>,
    /// Where the countdown on screen reports how it ended. Set only while a
    /// countdown is the current card.
    countdown: Option<Sender<CountdownEnd>>,
}

impl Slot {
    /// Show `card` until `closes_at_ms`, replacing the one on screen (a
    /// replaced countdown ends [`CountdownEnd::Closed`]). `ends` hears how a
    /// countdown ends. An `update_only` prompt (the same call noticed a second
    /// way) only replaces: with nothing on screen it opens nothing, and
    /// `None` comes back.
    pub fn show(
        &mut self,
        card: Card,
        closes_at_ms: i64,
        ends: Option<Sender<CountdownEnd>>,
    ) -> Option<PopupPrompt> {
        if let Card::Prompt { prompt } = &card
            && prompt.update_only
            && self.current.is_none()
        {
            return None;
        }
        self.finish(CountdownEnd::Closed);
        self.last_id = self.last_id.wrapping_add(1).max(1);
        let shown = PopupPrompt {
            id: self.last_id,
            card,
            closes_at_ms,
        };
        self.current = Some(shown.clone());
        self.countdown = ends;
        Some(shown)
    }

    /// The card on screen.
    pub fn current(&self) -> Option<PopupPrompt> {
        self.current.clone()
    }

    /// Card `id`'s time ran out: close it if it is still the one on screen.
    /// A prompt's timeout is "not now"; a countdown's is
    /// [`CountdownEnd::TimedOut`]. `true` when the window should hide.
    pub fn expire(&mut self, id: u32) -> bool {
        self.close_with(id, CountdownEnd::TimedOut)
    }

    /// Close card `id` if it is on screen, for any reason but an answer (a
    /// recording started elsewhere, a countdown cancelled).
    pub fn close_if(&mut self, id: u32) -> bool {
        self.close_with(id, CountdownEnd::Closed)
    }

    fn close_with(&mut self, id: u32, end: CountdownEnd) -> bool {
        if self.current.as_ref().is_some_and(|shown| shown.id == id) {
            self.finish(end);
            true
        } else {
            false
        }
    }

    /// Close the card on screen; a countdown reports `end`.
    fn finish(&mut self, end: CountdownEnd) {
        self.current = None;
        if let Some(ends) = self.countdown.take() {
            // The caller may have stopped listening; nothing to tell then.
            let _ = ends.send(end);
        }
    }

    /// The user pressed `answer` on card `id`. `None` when that card is no
    /// longer on screen (replaced, timed out, already answered): a second
    /// click on Record starts nothing. Every answer but Join closes it.
    pub fn answer(&mut self, id: u32, answer: PopupAnswer) -> Option<Action> {
        let shown = self.current.as_ref().filter(|shown| shown.id == id)?;
        match &shown.card {
            Card::Prompt { prompt } => {
                let action = action_for(prompt, answer);
                if answer != PopupAnswer::Join || matches!(action, Action::Nothing) {
                    self.current = None;
                }
                Some(action)
            }
            Card::Countdown { .. } => {
                self.finish(countdown::end_for(answer));
                Some(Action::Nothing)
            }
        }
    }
}

/// What `answer` on `prompt` does. A test reminder never records or joins.
pub fn action_for(prompt: &Prompt, answer: PopupAnswer) -> Action {
    if prompt.test {
        return Action::Nothing;
    }
    let event_id = prompt.event_id.clone();
    match answer {
        PopupAnswer::Dismiss | PopupAnswer::StopNow | PopupAnswer::KeepRecording => Action::Nothing,
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
        PopupAnswer::OpenBrief => match (&prompt.signal, &prompt.title) {
            (Signal::Calendar { .. }, Some(title)) => Action::OpenBrief {
                title: title.clone(),
            },
            _ => Action::Nothing,
        },
        PopupAnswer::NeverFor => match &prompt.app {
            Some(app) => Action::NeverFor { app: app.clone() },
            None => Action::Nothing,
        },
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

/// Does the card show `signal`'s prompt? Yes for every signal on every OS
/// (TUR-147, after TUR-108 did it for reminders); the notification is only
/// the fallback when the window cannot be shown.
pub fn uses_popup(signal: &Signal) -> bool {
    match signal {
        Signal::Calendar { .. } | Signal::Process { .. } | Signal::AudioActivity => true,
        Signal::Call { .. } => true,
    }
}

/// How showing a card went.
enum Shown {
    /// On screen, with this id.
    Up(u32),
    /// An `update_only` prompt with nothing on screen to update.
    Skipped,
    /// The window could not be made or shown.
    Failed,
}

/// Show `prompt` in the popup. `true` when the popup took it (shown, or an
/// `update_only` prompt with nothing on screen to update); `false` when the
/// window could not be made, so the caller falls back to a notification.
pub fn show(app: &AppHandle, prompt: &Prompt) -> bool {
    match present(app, prompt.clone().into(), AUTO_HIDE, None) {
        Shown::Up(id) => {
            tracing::debug!(id, "the prompt is up in its card");
            true
        }
        Shown::Skipped => true,
        Shown::Failed => false,
    }
}

/// Put `card` on screen for `lasts`, then close it ([`Slot::expire`]). The
/// window fades the card out from `closes_at_ms` on its own, and is hidden
/// [`FADE_OUT`] later.
fn present(
    app: &AppHandle,
    card: Card,
    lasts: Duration,
    ends: Option<Sender<CountdownEnd>>,
) -> Shown {
    let Some(state) = app.try_state::<PromptPopup>() else {
        tracing::warn!("the prompt popup is not set up; falling back to a notification");
        return Shown::Failed;
    };
    let layout = window::Layout::of(&card);
    let lasts_ms = i64::try_from(lasts.as_millis()).unwrap_or(i64::MAX);
    let closes_at_ms = chrono::Utc::now()
        .timestamp_millis()
        .saturating_add(lasts_ms);
    let Some(shown) = state.with(|slot| slot.show(card, closes_at_ms, ends)) else {
        return Shown::Skipped;
    };
    if let Err(error) = window::show(app, layout) {
        tracing::warn!(%error, "could not show the prompt popup; falling back to a notification");
        state.with(|slot| slot.close_if(shown.id));
        return Shown::Failed;
    }
    if let Err(error) = app.emit_to(window::LABEL, PROMPT_POPUP_EVENT, &shown) {
        tracing::warn!(%error, "could not send the prompt to its popup");
    }
    let app = app.clone();
    let id = shown.id;
    std::thread::spawn(move || {
        std::thread::sleep(lasts);
        let expired = app
            .try_state::<PromptPopup>()
            .is_some_and(|state| state.with(|slot| slot.expire(id)));
        if expired {
            tracing::debug!("the prompt popup timed out");
            hide_after_fade(&app);
        }
    });
    Shown::Up(id)
}

/// Hide the window once the card has faded out, unless a new card is up by
/// then.
fn hide_after_fade(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FADE_OUT);
        if app
            .try_state::<PromptPopup>()
            .is_none_or(|state| state.with(|slot| slot.current().is_none()))
        {
            window::hide(&app);
        }
    });
}

/// The card the popup should show, for a window that just loaded.
#[tauri::command]
#[specta::specta]
pub async fn prompt_popup_current(app: AppHandle) -> Option<PopupPrompt> {
    app.try_state::<PromptPopup>()
        .and_then(|state| state.with(|slot| slot.current()))
}

/// A popup button was pressed for card `id`.
#[tauri::command]
#[specta::specta]
pub async fn answer_prompt_popup(
    app: AppHandle,
    id: u32,
    answer: PopupAnswer,
) -> Result<(), UiError> {
    let shown = app
        .try_state::<PromptPopup>()
        .and_then(|state| state.with(|slot| slot.current()));
    // TUR-143: Not now on a call prompt starts that app's 10 minutes.
    let not_now = (answer == PopupAnswer::Dismiss)
        .then(|| shown.as_ref().and_then(|shown| not_now_app(shown, id)))
        .flatten();
    // TUR-144: the app a Record names, so its hang-up can end the recording.
    let origin = super::call_end::prompted_app(shown.as_ref(), id, answer);
    let action = app
        .try_state::<PromptPopup>()
        .and_then(|state| state.with(|slot| slot.answer(id, answer)));
    if let (Some(name), Some(detection)) = (not_now, app.try_state::<super::Detection>()) {
        detection.calls.dismissed(&name);
    }
    let Some(action) = action else {
        // Replaced or timed out; the window may still be up from a race.
        hide_after_fade(&app);
        return Ok(());
    };
    if !matches!(action, Action::Join { .. }) {
        hide_after_fade(&app);
    }
    run(&app, action).await?;
    super::call_end::started_from_prompt(&app, origin.as_deref());
    Ok(())
}

async fn run(app: &AppHandle, action: Action) -> Result<(), UiError> {
    match action {
        Action::Nothing => Ok(()),
        Action::NeverFor { app: name } => super::call_start::never_for(app, &name).await,
        Action::OpenBrief { title } => {
            crate::lifecycle::navigate(app, NavigateTo::Brief { title });
            Ok(())
        }
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

/// The app a Not now on card `id` is about: a call prompt's, when that card
/// is the one on screen (TUR-143).
pub fn not_now_app(shown: &PopupPrompt, id: u32) -> Option<String> {
    match &shown.card {
        Card::Prompt { prompt } if shown.id == id => match &prompt.signal {
            Signal::Call { app, .. } => Some(app.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests;
