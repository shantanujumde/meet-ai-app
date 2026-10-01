//! The `---` YAML block at the top of `meeting.md` and `TICK-NNNN.md`.
//!
//! SETUP.md §1.2: parse to an ordered `yaml-rust2` value, mutate only the keys
//! this code owns, emit. Unknown keys survive because they are never modelled
//! — there is deliberately no struct here to deserialize into.
//!
//! What survives a round trip is the *values*: every key, its position, and
//! nested maps and lists. What does not survive is the *spelling*, because
//! `yaml-rust2` keeps no record of it:
//!
//! * YAML comments inside the block are dropped (the scanner discards them),
//!   so the `# optional` notes in the SPEC §3.2 example vanish on first write.
//! * Quoting and list style are re-chosen on output: `[a, b]` comes back as a
//!   block list, and a value with `:` in it (timestamps, `00:14:22`) comes back
//!   double-quoted whether or not it was quoted before.
//! * Numbers and booleans are normalised: `0x1F` is written back as `31`,
//!   `True` as `true`. An anchor (`&name`) is dropped on write.
//!
//! Two shapes are refused as `BadFrontmatter` rather than loaded: any alias
//! (`*name`), and nesting deeper than 64 levels. Both are ways a tiny file can
//! take the whole app down — see `MAX_DEPTH` and `check_shape`.

mod fields;
mod parse;
mod write;

#[cfg(test)]
mod tests;

pub use fields::{Document, Frontmatter};
pub use parse::{parse, split};
pub use write::render;
