/**
 * Values the window shows in more than one place.
 *
 * Each of these was a literal copied across screens. They are display strings
 * and timings, not configuration: the real meetings root and the real shortcut
 * are Rust's to decide, and these only describe them until Rust answers.
 */

import type {
  meet_ai_lib_config_appearance_section_AppearanceConfig as Appearance,
  meet_ai_lib_lifecycle_AppSettings as AppSettings,
  meet_ai_lib_calendar_TodaysMeetings as TodaysMeetings,
} from "@/ipc/bindings";

/**
 * The meetings folder as written before `list_meetings` has answered, or when
 * there is no backend. Matches Rust's default root (`config.rs`).
 */
export const DEFAULT_ROOT_LABEL = "~/Meetings";

/** How long a "Copied" confirmation stays before the button reads normally again. */
export const COPIED_RESET_MS = 2000;

/**
 * An empty day at the config defaults (`calendar.refresh_minutes` 15,
 * `detection.min_attendees` 2, as in `config/`): what `todaysMeetings` answers
 * with no backend, and what the Today pane times its re-reads by until Rust
 * says otherwise.
 */
export const NO_MEETINGS_TODAY: TodaysMeetings = {
  events: [],
  refreshMinutes: 15,
  minAttendees: 2,
  unreadable: [],
};

/**
 * The `app` section's defaults (TUR-76), for when there is no Rust side to
 * ask: the Dock icon goes with the window.
 */
export const DEFAULT_APP_SETTINGS: AppSettings = { showInDockWhenClosed: false };

/** `app.menu_bar_countdown`'s default (TUR-77): no countdown next to the icon. */
export const DEFAULT_MENU_BAR_COUNTDOWN = false;

/** `appearance`'s defaults (TUR-102): follow the OS, with the see-through glass on. */
export const DEFAULT_APPEARANCE: Appearance = { theme: "system", glass: true };
