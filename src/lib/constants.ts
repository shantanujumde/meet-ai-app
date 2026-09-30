/**
 * Values the window shows in more than one place.
 *
 * Each of these was a literal copied across screens. They are display strings
 * and timings, not configuration: the real meetings root and the real shortcut
 * are Rust's to decide, and these only describe them until Rust answers.
 */

/**
 * The meetings folder as written before `list_meetings` has answered, or when
 * there is no backend. Matches Rust's default root (`config.rs`).
 */
export const DEFAULT_ROOT_LABEL = "~/Meetings";

/** The global start/stop shortcut, as macOS writes it. Registered in Rust (`lib.rs`). */
export const SHORTCUT_LABEL = "⌘⇧R";

/** How long a "Copied" confirmation stays before the button reads normally again. */
export const COPIED_RESET_MS = 2000;
