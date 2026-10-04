//! Which picture the tray shows (TUR-58).
//!
//! macOS keeps its template glyph in every state: AppKit recolours it, and the
//! menu bar already says "Stop recording" in the menu. Windows and Linux do
//! not recolour anything, so they get filled glyphs made from the same art by
//! `scripts/make-tray-icons.py`: light or dark by the Windows taskbar theme,
//! the brand ember on Linux (readable on light and dark panels alike, and
//! there is no one setting to read), and red while recording on both.
//!
//! The choice is plain data so every OS's answer is tested on every OS; the
//! platform module only says which OS this is and how the taskbar looks.

// Adapted from github.com/cjpais/Handy/src-tauri/src/tray.rs @ 73ab851c2b6242283759a4c101b60f0ece132f08 (MIT)
// (the theme-by-OS icon pick: template on macOS, by taskbar theme on Windows,
// always coloured on Linux; rewritten for our states and art).

/// The menu-bar glyph: "m.", the brand symbol, drawn on the 16px pixel grid by
/// the brand build (`design-system/meet-ai/brand/tools/build.mjs`) and
/// rasterised by its `render.sh`. Never hand-exported. macOS recolours it as a
/// template; the other files here are filled copies of it.
///
/// The 2x raster is the one embedded. `tray-icon` sizes every status-item image
/// to 18pt tall whatever it is handed, so the larger art is the one with pixels
/// to spare when AppKit scales it.
///
/// Embedded rather than bundled as a resource: a resource that fails to copy
/// leaves a menu-bar item with no icon at all, and that is a far worse failure
/// than a slightly bigger binary.
const TEMPLATE: &[u8] = include_bytes!("../../icons/meet-aiTemplate@2x.png");
const CHALK: &[u8] = include_bytes!("../../icons/tray-chalk.png");
const INK: &[u8] = include_bytes!("../../icons/tray-ink.png");
const EMBER: &[u8] = include_bytes!("../../icons/tray-ember.png");
const RECORDING: &[u8] = include_bytes!("../../icons/tray-recording.png");

/// The tray's OS family, as far as the icon cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "each OS build uses only its own family")
)]
pub enum TrayOs {
    MacOs,
    Windows,
    Linux,
}

/// One tray picture: the PNG bytes, and whether macOS should recolour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayArt {
    pub png: &'static [u8],
    pub template: bool,
}

/// The picture for `os`, a taskbar that is light or not (Windows only), and
/// whether a recording is under way (any phase but idle).
pub fn pick(os: TrayOs, light_taskbar: bool, recording: bool) -> TrayArt {
    let png = match (os, recording) {
        (TrayOs::MacOs, _) => {
            return TrayArt {
                png: TEMPLATE,
                template: true,
            };
        }
        (_, true) => RECORDING,
        (TrayOs::Windows, false) if light_taskbar => INK,
        (TrayOs::Windows, false) => CHALK,
        (TrayOs::Linux, false) => EMBER,
    };
    TrayArt {
        png,
        template: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_keeps_the_template_glyph_in_every_state() {
        for light in [true, false] {
            for recording in [true, false] {
                let art = pick(TrayOs::MacOs, light, recording);
                assert_eq!(art.png, TEMPLATE);
                assert!(art.template);
            }
        }
    }

    #[test]
    fn windows_picks_by_taskbar_theme_when_idle() {
        // A dark taskbar (the Windows 11 default) needs the light glyph.
        assert_eq!(pick(TrayOs::Windows, false, false).png, CHALK);
        assert_eq!(pick(TrayOs::Windows, true, false).png, INK);
        assert!(!pick(TrayOs::Windows, false, false).template);
    }

    #[test]
    fn linux_is_always_coloured() {
        assert_eq!(pick(TrayOs::Linux, false, false).png, EMBER);
        assert_eq!(pick(TrayOs::Linux, true, false).png, EMBER);
    }

    #[test]
    fn recording_shows_the_red_glyph_off_macos() {
        for os in [TrayOs::Windows, TrayOs::Linux] {
            for light in [true, false] {
                assert_eq!(pick(os, light, true).png, RECORDING, "{os:?}");
            }
        }
    }

    #[test]
    fn every_icon_is_a_png() {
        for png in [TEMPLATE, CHALK, INK, EMBER, RECORDING] {
            assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }
}
