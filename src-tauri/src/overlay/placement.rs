//! Where the overlay window goes (TUR-146): its default spot, and whether
//! the spot it was dragged to last time is still on a screen. Pure, so every
//! OS's answer is tested on every OS; `window.rs` feeds it the monitors.

use serde::{Deserialize, Serialize};

/// The overlay's size, in logical pixels: two short lines and three
/// buttons, small enough to sit over a call without covering it.
pub const WIDTH: f64 = 320.0;
pub const HEIGHT: f64 = 90.0;
/// Its gap from the work area's top and right edges, in logical pixels.
pub const MARGIN: f64 = 16.0;
/// The card's corner radius, which the macOS material is cut to as well:
/// `--radius-panel` in `design-system/meet-ai/tokens.css`.
pub const RADIUS: f64 = 16.0;
/// Room left above the default spot for the prompt card
/// (`detection/popup/window.rs`, at the same margin): the taller of its
/// two shapes, the 120 px narrow card that TUR-144's and TUR-145's stop
/// countdowns use mid-recording, so no card lands on Pause and Stop.
pub const CARD_SLOT: f64 = 120.0;

/// A window's top-left corner, in physical pixels: what the OS reports
/// and what `state.json` keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// A monitor's area, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Area {
    fn contains(&self, point: Point) -> bool {
        let (x, y) = (i64::from(point.x), i64::from(point.y));
        let (left, top) = (i64::from(self.x), i64::from(self.y));
        x >= left
            && y >= top
            && x < left + i64::from(self.width)
            && y < top + i64::from(self.height)
    }
}

/// The default spot: top-right of `work_area` (the monitor less the menu
/// bar, Dock or taskbar), under the reminder card's slot, at `scale`.
pub fn default_spot(work_area: Area, scale: f64) -> Point {
    let width = (WIDTH * scale).round() as i64;
    let margin = (MARGIN * scale).round() as i64;
    let below_card = ((MARGIN + CARD_SLOT + MARGIN) * scale).round() as i64;
    let right = i64::from(work_area.x) + i64::from(work_area.width);
    let x = (right - width - margin).max(i64::from(work_area.x));
    let y = i64::from(work_area.y) + below_card;
    Point {
        x: clamp_i32(x),
        y: clamp_i32(y),
    }
}

/// Where the overlay opens: the `saved` spot when its top-left corner still
/// lands on one of `monitors` (a screen unplugged since would leave it out of
/// reach), else `None` for the default spot.
pub fn restore(saved: Option<Point>, monitors: &[Area]) -> Option<Point> {
    saved.filter(|point| monitors.iter().any(|area| area.contains(*point)))
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: Area = Area {
        x: 0,
        y: 0,
        width: 3024,
        height: 1964,
    };

    #[test]
    fn the_default_spot_is_top_right_under_the_reminder_card() {
        // A Retina laptop's work area under a 37 px menu bar, at 2x.
        let area = Area { y: 74, ..LAPTOP };
        let spot = default_spot(area, 2.0);
        assert_eq!(spot.x, 3024 - 640 - 32, "flush right, less the margin");
        assert_eq!(spot.y, 74 + 304, "under the card's slot");
    }

    #[test]
    fn a_work_area_narrower_than_the_overlay_keeps_it_on_screen() {
        let narrow = Area {
            x: 100,
            y: 0,
            width: 200,
            height: 400,
        };
        assert_eq!(default_spot(narrow, 1.0).x, 100);
    }

    #[test]
    fn a_saved_spot_on_a_screen_is_kept() {
        let external = Area {
            x: -2560,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let saved = Point { x: -400, y: 30 };
        assert_eq!(restore(Some(saved), &[LAPTOP, external]), Some(saved));
    }

    #[test]
    fn a_saved_spot_off_every_screen_falls_back_to_the_default() {
        let gone = Point { x: -400, y: 30 };
        assert_eq!(restore(Some(gone), &[LAPTOP]), None, "screen unplugged");
        assert_eq!(restore(None, &[LAPTOP]), None, "never dragged");
        assert_eq!(restore(Some(Point { x: 10, y: 10 }), &[]), None);
    }
}
