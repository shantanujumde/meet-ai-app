/**
 * Values the window shows in more than one place.
 *
 * Each of these was a literal copied across screens. The config defaults come
 * from Rust (`DEFAULT_*` in `bindings.ts`, TUR-173); the rest are display
 * strings and timings: the real meetings root and the real shortcut are
 * Rust's to decide, and these only describe them until Rust answers.
 */

import {
  type meet_ai_lib_config_appearance_section_AppearanceConfig as Appearance,
  type meet_ai_lib_lifecycle_AppSettings as AppSettings,
  DEFAULT_TODAYS_MEETINGS,
  DEFAULT_APP_SETTINGS as RUST_APP_SETTINGS,
  DEFAULT_APPEARANCE as RUST_APPEARANCE,
  DEFAULT_MENU_BAR_COUNTDOWN as RUST_MENU_BAR_COUNTDOWN,
  type meet_ai_lib_calendar_TodaysMeetings as TodaysMeetings,
} from "@/ipc/bindings";
import { writable } from "./writable";

/**
 * The meetings folder as written before `list_meetings` has answered, or when
 * there is no backend. Matches Rust's default root (`config.rs`).
 */
export const DEFAULT_ROOT_LABEL = "~/Meetings";

/** How long a "Copied" confirmation stays before the button reads normally again. */
export const COPIED_RESET_MS = 2000;

/**
 * An empty day at the config defaults (`calendar.refresh_minutes`,
 * `detection.min_attendees`): what `todaysMeetings` answers with no backend,
 * and what the Today pane times its re-reads by until Rust says otherwise.
 */
export const NO_MEETINGS_TODAY: TodaysMeetings = writable(DEFAULT_TODAYS_MEETINGS);

/**
 * The `app` section's defaults (TUR-76), for when there is no Rust side to
 * ask: the Dock icon goes with the window.
 */
export const DEFAULT_APP_SETTINGS: AppSettings = writable(RUST_APP_SETTINGS);

/** `app.menu_bar_countdown`'s default (TUR-77): no countdown next to the icon. */
export const DEFAULT_MENU_BAR_COUNTDOWN: boolean = RUST_MENU_BAR_COUNTDOWN;

/** `appearance`'s defaults (TUR-102): follow the OS, with the see-through glass on. */
export const DEFAULT_APPEARANCE: Appearance = writable(RUST_APPEARANCE);
