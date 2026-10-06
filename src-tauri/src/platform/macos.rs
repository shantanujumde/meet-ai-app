//! macOS-only pieces of the app shell (SPEC §8.2, rule R10).

use objc2_foundation::{NSDateFormatter, NSLocale, NSString};

/// The user's 12/24-hour clock (TUR-129): the current locale's pattern for
/// the `j` (preferred hour) template, which follows System Settings' "24-hour
/// time" switch. An AM/PM marker in it means 12-hour.
pub fn clock() -> crate::tray::Clock {
    let locale = NSLocale::currentLocale();
    let template = NSString::from_str("j");
    NSDateFormatter::dateFormatFromTemplate_options_locale(&template, 0, Some(&locale))
        .map(|pattern| crate::tray::Clock::from_icu_pattern(&pattern.to_string()))
        .unwrap_or(crate::tray::Clock::H24)
}
