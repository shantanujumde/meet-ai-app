//! The countdown card (TUR-147): "Zoom call ended", a ring counting down,
//! **Stop now** and **Keep recording**, in the same window as the prompts.
//!
//! This is the API TUR-144 (the call ended) and TUR-145 (10 min of silence)
//! call; it holds no stop logic of its own. [`show_countdown`] puts the card
//! up and hands back a [`CountdownHandle`], which says how it ended: the
//! user's button, the time running out, or the card closing some other way
//! (replaced by another card, or [`CountdownHandle::cancel`], for "the app
//! uses the mic again, cancel quietly"). Only [`CountdownEnd::StopNow`] and
//! [`CountdownEnd::TimedOut`] mean stop; anything else keeps recording, so a
//! card nobody could see never stops a recording.
//!
//! Unlike a prompt it does not hide after `AUTO_HIDE`: it ends when the
//! countdown does.

// TUR-144 and TUR-145 add the callers; until then only the tests use it.
#![allow(dead_code, reason = "TUR-144 and TUR-145 add the callers")]

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use tauri::{AppHandle, Manager as _};

use super::{Card, PopupAnswer, PromptPopup, Shown};

/// How a countdown card ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownEnd {
    /// The user pressed **Stop now**.
    StopNow,
    /// The user pressed **Keep recording**.
    KeepRecording,
    /// Nobody answered before it reached zero.
    TimedOut,
    /// It closed without an answer: replaced by another card, cancelled, or
    /// closed because the recording already stopped. Keep recording.
    Closed,
}

impl CountdownEnd {
    /// Does this end mean "stop the recording"?
    pub fn stops(self) -> bool {
        matches!(self, Self::StopNow | Self::TimedOut)
    }
}

/// What a button on the countdown card means. Any answer but the two
/// countdown buttons (a stale window's Dismiss, say) closes it quietly.
pub fn end_for(answer: PopupAnswer) -> CountdownEnd {
    match answer {
        PopupAnswer::StopNow => CountdownEnd::StopNow,
        PopupAnswer::KeepRecording => CountdownEnd::KeepRecording,
        PopupAnswer::Record
        | PopupAnswer::JoinAndRecord
        | PopupAnswer::Join
        | PopupAnswer::OpenBrief
        | PopupAnswer::Dismiss
        | PopupAnswer::NeverFor => CountdownEnd::Closed,
    }
}

/// A countdown card on screen.
#[derive(Debug)]
pub struct CountdownHandle {
    app: AppHandle,
    id: u32,
    ends: Receiver<CountdownEnd>,
}

impl CountdownHandle {
    /// Block until the card ends.
    pub fn wait(&self) -> CountdownEnd {
        self.ends.recv().unwrap_or(CountdownEnd::Closed)
    }

    /// How the card ended, or `None` while it is still up.
    pub fn try_end(&self) -> Option<CountdownEnd> {
        match self.ends.try_recv() {
            Ok(end) => Some(end),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(CountdownEnd::Closed),
        }
    }

    /// Close the card quietly, if it is still up. It ends
    /// [`CountdownEnd::Closed`].
    pub fn cancel(&self) {
        let closed = self
            .app
            .try_state::<PromptPopup>()
            .is_some_and(|state| state.with(|slot| slot.close_if(self.id)));
        if closed {
            super::hide_after_fade(&self.app);
        }
    }
}

/// Show the countdown card: `line` ("Zoom call ended") over a ring counting
/// `seconds` down to zero. `None` when the window could not be shown; the
/// caller decides what that means (it should not stop a recording the user
/// was never asked about).
pub fn show_countdown(app: &AppHandle, line: &str, seconds: u32) -> Option<CountdownHandle> {
    let (tell, ends) = mpsc::channel();
    let card = Card::Countdown {
        line: line.to_string(),
        seconds,
    };
    let lasts = Duration::from_secs(u64::from(seconds));
    match super::present(app, card, lasts, Some(tell)) {
        Shown::Up(id) => Some(CountdownHandle {
            app: app.clone(),
            id,
            ends,
        }),
        Shown::Skipped | Shown::Failed => None,
    }
}
