//! The board: the pane's model kept in Rust, and the scope an engine thread
//! writes it through.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use stt::{LiveLine, LiveUpdate, SeqCounter, Speaker};

use super::{Notify, State, Status};
use crate::lock::lock_or_recover;

/// How long a guess may sit on screen, in the track's own audio, with the
/// engine neither updating, settling nor withdrawing it.
///
/// Apple's model guesses at room tone (one "I", measured on
/// `room-tone-30s.wav`) and then says nothing until the stream ends, which
/// would leave a phantom "still speaking" line up for a whole quiet stretch —
/// the live form of the hallucination TUR-67 guards against. Longer than the
/// ~4 s Apple takes to settle a real utterance after it ends (TUR-31), so a
/// genuine guess is replaced by its final line, not withdrawn first; and if
/// one ever is, the final still lands, because a final always appends.
pub(super) const STALE_GUESS: Duration = Duration::from_secs(6);

/// The pane's model, kept in Rust: settled lines plus one tail per speaker.
///
/// The same three rules the pane follows (`stt::session`'s tail contract), so
/// a snapshot and a replay of every event always agree.
#[derive(Debug, Default)]
pub(super) struct Lines {
    pub(super) finals: Vec<LiveLine>,
    you: Option<LiveLine>,
    others: Option<LiveLine>,
}

impl Lines {
    pub(super) fn tail(&mut self, speaker: Speaker) -> &mut Option<LiveLine> {
        match speaker {
            Speaker::You => &mut self.you,
            Speaker::Others => &mut self.others,
        }
    }

    pub(super) fn apply(&mut self, update: &LiveUpdate) {
        match update {
            LiveUpdate::Volatile(line) => *self.tail(line.speaker) = Some(line.clone()),
            LiveUpdate::Final(line) => {
                *self.tail(line.speaker) = None;
                self.finals.push(line.clone());
            }
            LiveUpdate::Dropped { speaker, .. } => *self.tail(*speaker) = None,
        }
    }

    pub(super) fn volatile(&self) -> Vec<LiveLine> {
        self.you.iter().chain(&self.others).cloned().collect()
    }
}

/// The state [`LiveTranscript::snapshot`] reads and the engine threads write.
#[derive(Debug)]
pub(super) struct Board {
    /// Bumped at every meeting start and when a meeting is sealed. A thread
    /// only writes while this still matches the value it started with, so a
    /// wedged engine abandoned at Stop cannot scribble on the next meeting.
    pub(super) generation: u64,
    pub(super) status: Status,
    pub(super) lines: Lines,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            generation: 0,
            status: Status::idle(),
            lines: Lines::default(),
        }
    }
}

/// Everything an engine thread needs to write to the board and the window
/// safely: which board, which meeting, and where events go.
#[derive(Clone)]
pub(super) struct Scope {
    pub(super) board: Arc<Mutex<Board>>,
    pub(super) generation: u64,
    pub(super) notify: Arc<dyn Notify>,
}

impl Scope {
    /// Change the board if this meeting still owns it. The lock is released
    /// before anything is emitted, so a slow window never holds up an engine
    /// thread's peer or the snapshot command.
    pub(super) fn with_board<T>(&self, change: impl FnOnce(&mut Board) -> T) -> Option<T> {
        let mut board = lock_or_recover(&self.board);
        (board.generation == self.generation).then(|| change(&mut board))
    }

    /// The listener body. Called from engine threads, never blocks.
    pub(super) fn update(&self, update: &LiveUpdate) {
        if self.with_board(|board| board.lines.apply(update)).is_some() {
            self.notify.update(update);
        }
    }

    pub(super) fn set_status(&self, status: Status) {
        let changed = self.with_board(|board| {
            // A failure is the last word on a meeting: a track that finishes
            // cleanly after its partner failed must not paper over it.
            if board.status.state == State::Failed {
                return None;
            }
            // Frames already lost stay counted whatever the new status says.
            let dropped_frames = board.status.dropped_frames;
            board.status = Status {
                dropped_frames,
                ..status
            };
            Some(board.status.clone())
        });
        if let Some(Some(status)) = changed {
            self.notify.status(&status);
        }
    }

    pub(super) fn running(&self, engine: &str) {
        self.set_status(Status {
            state: State::Running,
            engine: Some(engine.to_string()),
            detail: None,
            dropped_frames: 0,
        });
    }

    /// A track's engine never got `frames` of its audio (TUR-148). Counted on
    /// the board, which `settle` leaves alone, so the status Stop returns
    /// says the transcript has gaps even when it ended `stopped`.
    pub(super) fn dropped(&self, frames: u64) {
        if frames == 0 {
            return;
        }
        let _ = self.with_board(|board| {
            board.status.dropped_frames = board.status.dropped_frames.saturating_add(frames);
        });
    }

    /// Stop with a reason. Logged here, so every failure path is logged once.
    /// Only the first failure is reported: the second is almost always the
    /// first one's consequence.
    pub(super) fn fail(&self, detail: String) {
        let changed = self.with_board(|board| {
            if board.status.state == State::Failed {
                return None;
            }
            board.status.state = State::Failed;
            board.status.detail = Some(detail);
            Some(board.status.clone())
        });
        if let Some(Some(status)) = changed {
            tracing::warn!(
                detail = status.detail.as_deref().unwrap_or_default(),
                "live transcription stopped"
            );
            self.notify.status(&status);
        }
    }

    /// The meeting ended normally. A failure already on the board stays, and
    /// is sent again so the window's last word is the true one either way.
    pub(super) fn settle(&self) {
        let status = self.with_board(|board| {
            if board.status.state != State::Failed {
                board.status.state = State::Stopped;
                board.status.detail = None;
            }
            board.status.clone()
        });
        if let Some(status) = status {
            self.notify.status(&status);
        }
    }

    /// Withdraw a guess its engine has left unchanged for [`STALE_GUESS`] of
    /// this track's audio.
    ///
    /// `guess` is this track's memory of which guess it has been watching
    /// and how far into the audio it first saw it. The check and the
    /// withdrawal happen under one lock, so a guess the engine replaces in
    /// the meantime is never the one taken down.
    pub(super) fn expire_stale_guess(
        &self,
        speaker: Speaker,
        fed: u64,
        guess: &mut Option<(u64, u64)>,
        seq: &SeqCounter,
    ) {
        let stale_after = STALE_GUESS.as_secs() * u64::from(stt::vad::SAMPLE_RATE);
        let dropped = self.with_board(|board| {
            let showing = board.lines.tail(speaker).as_ref().map(|line| line.seq);
            match (showing, *guess) {
                (None, _) => {
                    *guess = None;
                    None
                }
                (Some(now), Some((watched, since))) if now == watched => {
                    if fed.saturating_sub(since) < stale_after {
                        return None;
                    }
                    let update = LiveUpdate::Dropped {
                        speaker,
                        seq: seq.next(),
                    };
                    board.lines.apply(&update);
                    *guess = None;
                    Some(update)
                }
                (Some(now), _) => {
                    *guess = Some((now, fed));
                    None
                }
            }
        });
        if let Some(Some(update)) = dropped {
            tracing::debug!(
                speaker = speaker.label(),
                "withdrew a guess the engine never settled"
            );
            self.notify.update(&update);
        }
    }

    pub(super) fn is_settled(&self) -> bool {
        self.with_board(|board| matches!(board.status.state, State::Stopped | State::Failed))
            .unwrap_or(true)
    }

    /// Take the meeting's ownership of the board away, keeping what it shows.
    /// Anything an abandoned thread tries to write afterwards is ignored.
    pub(super) fn seal(&self) -> Status {
        let mut board = lock_or_recover(&self.board);
        if board.generation == self.generation {
            board.generation += 1;
        }
        board.status.clone()
    }
}
