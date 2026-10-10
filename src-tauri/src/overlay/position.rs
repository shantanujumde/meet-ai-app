//! Where the user last dragged the overlay (TUR-146), kept in
//! `~/Meetings/.app/state.json` beside the still-running flag
//! (`lifecycle::notice`), never in the webview's storage: off macOS the
//! window is made afresh for every recording, and the app writes nothing
//! outside the meetings root (L10). Saved whenever the window is taken down,
//! hidden or closed (TUR-180). Every other key in the file is kept on a
//! write.

use std::path::Path;

use crate::error::UiError;
use crate::lifecycle::notice::{self, AppState};

use super::placement::Point;

/// The key in `state.json`.
const KEY: &str = "overlayPosition";

/// The spot saved in `state`, if there is a readable one.
pub fn saved_in(state: &AppState) -> Option<Point> {
    let value = state.other.get(KEY)?;
    serde_json::from_value(value.clone())
        .inspect_err(
            |error| tracing::debug!(%error, "unreadable overlay position; using the default"),
        )
        .ok()
}

/// `state` with `point` saved.
pub fn with_saved(mut state: AppState, point: Point) -> Result<AppState, UiError> {
    let value = serde_json::to_value(point)
        .map_err(|error| UiError::app("serialize", error.to_string()))?;
    state.other.insert(KEY.to_string(), value);
    Ok(state)
}

/// The spot saved under the meetings root `root`.
pub fn load(root: &Path) -> Option<Point> {
    saved_in(&notice::read_in(&meeting_format::layout::app_dir(root)))
}

/// Save `point` under the meetings root `root`.
pub fn save(root: &Path, point: Point) -> Result<(), UiError> {
    let dir = meeting_format::layout::app_dir(root);
    notice::write_in(&dir, &with_saved(notice::read_in(&dir), point)?)
}

#[cfg(test)]
mod tests {
    use super::super::placement::{self, Area};
    use super::*;

    #[test]
    fn a_saved_spot_comes_back_and_the_other_keys_stay() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        assert_eq!(load(root), None, "never dragged");

        let dir = meeting_format::layout::app_dir(root);
        let flagged = AppState {
            still_running_notice_shown: true,
            ..AppState::default()
        };
        notice::write_in(&dir, &flagged).unwrap();

        save(root, Point { x: -400, y: 30 }).unwrap();
        assert_eq!(load(root), Some(Point { x: -400, y: 30 }));
        assert!(
            notice::read_in(&dir).still_running_notice_shown,
            "the still-running flag survives"
        );
        save(root, Point { x: 5, y: 6 }).unwrap();
        assert_eq!(load(root), Some(Point { x: 5, y: 6 }));
    }

    #[test]
    fn the_spot_saved_when_it_is_hidden_is_where_it_shows_next() {
        let temp = tempfile::tempdir().unwrap();
        let screen = Area {
            x: 0,
            y: 0,
            width: 3440,
            height: 1440,
        };
        save(temp.path(), Point { x: 2900, y: 400 }).unwrap();
        assert_eq!(
            placement::restore(load(temp.path()), &[screen]),
            Some(Point { x: 2900, y: 400 })
        );
    }

    #[test]
    fn an_unreadable_spot_is_no_spot() {
        let mut state = AppState::default();
        state
            .other
            .insert(KEY.to_string(), serde_json::json!({ "x": "left" }));
        assert_eq!(saved_in(&state), None);
    }
}
