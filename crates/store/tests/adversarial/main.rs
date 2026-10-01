//! Adversarial tests for `store`: hostile, generated and plain weird input,
//! through the public API only.
//!
//! `fixture_folder.rs` checks the TUR-99 gate over a realistic folder. This
//! file checks the edges around it: that no parser panics, that a write is
//! stable (writing what was read, then reading and writing again, gives the
//! same bytes), that the folder scan survives a hostile tree, that nothing
//! unreadable is ever overwritten, and that an id can never leave the root.
//!
//! Generated cases come from a small fixed-seed xorshift generator, so every
//! run sees the same inputs and a failure message's input can be replayed.
//! The workspace pins every dependency, so there is no proptest here.
//!
//! Five tests here started life as failing repros of real defects (a stack
//! overflow on deep nesting, alias expansion, a BOM hiding a transcript line,
//! a NUL in an id, a leftover temp file) and now guard the fixes.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use store::folder::{self, MeetingFolder};
use store::frontmatter::{self, Document, Frontmatter};
use store::meeting::{Meeting, SECTIONS};
use store::ticket::Ticket;
use store::transcript::{self, Speaker};
use store::{Error, MEETING_FILE, NOTES_FILE, Problem, TICKETS_DIR, TRANSCRIPT_FILE, notes};
use yaml_rust2::Yaml;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// xorshift64: tiny, deterministic, good enough to shuffle test input.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // xorshift never leaves zero, so never start there.
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// `0..n`. `n` must be above zero.
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// A scratch folder under the OS temp dir, unique to this test and process,
/// removed on drop.
#[allow(dead_code)] // field 1 keeps the directory alive until drop
struct Scratch(PathBuf, tempfile::TempDir);

impl Scratch {
    fn new(name: &str) -> Self {
        let guard = tempfile::Builder::new()
            .prefix(&format!("meet-ai-store-adversarial-{name}-"))
            .tempdir()
            .unwrap();
        Self(guard.path().to_path_buf(), guard)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

/// Every dotfile in `dir` — where `write_atomic` would leave a temp file.
fn dotfiles(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// Would a write of something that loaded with these problems be refused?
fn is_refused(problems: &[Problem]) -> bool {
    problems.iter().any(|p| {
        matches!(
            p,
            Problem::BadFrontmatter { .. } | Problem::Unreadable { .. }
        )
    })
}

/// A problem as a short comparable label, e.g. `MissingField(id)`.
fn label(problem: &Problem) -> String {
    match problem {
        Problem::Unreadable { .. } => "Unreadable".into(),
        Problem::NoFrontmatter => "NoFrontmatter".into(),
        Problem::BadFrontmatter { .. } => "BadFrontmatter".into(),
        Problem::MissingField { key } => format!("MissingField({key})"),
        Problem::BadField { key, .. } => format!("BadField({key})"),
        Problem::MissingSection { heading } => format!("MissingSection({heading})"),
        Problem::UnparsedLines { count } => format!("UnparsedLines({count})"),
    }
}

/// A folder's problems as `(file, label)` pairs, in order.
fn labelled(folder: &MeetingFolder) -> Vec<(String, String)> {
    folder
        .problems
        .iter()
        .map(|p| (p.file.clone(), label(&p.problem)))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(f, l)| ((*f).to_owned(), (*l).to_owned()))
        .collect()
}

/// SPEC §3.4's write rule, spelled out independently of the crate.
fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every problem except `NoFrontmatter` — what a file should report after
/// one write puts an (empty) block on top of it.
fn without_no_frontmatter(problems: &[Problem]) -> Vec<Problem> {
    problems
        .iter()
        .filter(|p| **p != Problem::NoFrontmatter)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Input generation
// ---------------------------------------------------------------------------

/// The nasty building blocks. Some appear twice to make duplicate keys and
/// repeated headings likely.
const PIECES: &[&str] = &[
    "---",
    "---\n",
    "---\n",
    "--- \n",
    "---\r\n",
    "----\n",
    "\n",
    "\n",
    "\r\n",
    "\r",
    "\u{feff}",
    "## ",
    "## Summary\n",
    "## Decisions\n",
    "## Action Items\r\n",
    "## Open Questions",
    "##   spaced   \n",
    "##\n",
    "###",
    "### Detail\n",
    "#",
    "# ",
    "\t",
    "\0",
    "\u{7}",
    "\u{1b}",
    "🎉",
    "👩‍👩‍👧",
    "é",
    "e\u{301}",
    "中文",
    "\u{2028}",
    "\u{85}",
    "\u{200b}",
    "\u{a0}",
    " ",
    "  ",
    "id: m1\n",
    "id: m1\n",
    "title: T\n",
    "status: open\n",
    "status: maybe\n",
    "a: 1\n",
    "a: 1\n",
    "&a ",
    "*a",
    "&a [1, 2]\n",
    "b: *a\n",
    "!!binary ",
    "!!binary aGVsbG8=\n",
    "!!int ",
    "!!str ",
    "!custom ",
    "? ",
    "|",
    "|-\n  ",
    ">",
    ">\n  folded\n",
    "{",
    "}",
    "[",
    "]",
    "'",
    "\"",
    "%YAML 1.2\n",
    "%TAG ! tag:x,2026:\n",
    "...",
    "...\n",
    "- ",
    "- x\n",
    ":",
    ": ",
    ",",
    "#comment",
    "0o17",
    "+.inf",
    "~",
    "null",
    "k: [1, 2]\n",
    "k: {x: y}\n",
    "[00:00:01] You: ",
    "[00:00:01] Others: hi\n",
    "[99:59:59] ",
    "[0:00:01] You: x\n",
    "You:",
    "Others:",
    "\\",
    "\\u",
    "x",
    "value",
];

/// One very long line. Units that nest YAML when repeated (`- `, `: `,
/// `[`) are left out on purpose: a few thousand of those overflow the stack,
/// which has its own (ignored) test below.
fn long_line(rng: &mut Rng) -> String {
    let unit = *rng.pick(&["a", "é", "🎉", "## ", ":", "x ", "\t", "中", "] "]);
    unit.repeat(1 + rng.below(20_000))
}

fn nasty_text(rng: &mut Rng, max_pieces: usize) -> String {
    let n = rng.below(max_pieces + 1);
    let mut out = String::new();
    for _ in 0..n {
        if rng.one_in(150) {
            out.push_str(&long_line(rng));
        } else {
            out.push_str(rng.pick(PIECES));
        }
    }
    out
}

const YAML_KEYS: &[&str] = &[
    "id",
    "title",
    "status",
    "meeting",
    "mood",
    "a",
    "b",
    "日本",
    "\"quoted key\"",
    "'single'",
    "with space",
    "? complex",
    "~",
    "1",
    "true",
];

const YAML_VALUES: &[&str] = &[
    "m1",
    "T",
    "open",
    "\"00:14:22\"",
    "00:14:22",
    "[a, b]",
    "[]",
    "{x: 1, y: [1, 2.50, true, null]}",
    "{}",
    "~",
    "null",
    "2026-09-01",
    "2026-09-01T14:30:00+05:30",
    "0x1F",
    "0o17",
    "+.inf",
    ".nan",
    "1e5",
    "'single ''quoted'''",
    "\"tab\\there \\u00e9 \\\"q\\\"\"",
    "&anc 5",
    "*anc",
    "!!str 12",
    "!!int abc",
    "!!binary aGVsbG8=",
    "!custom thing",
    "|\n  block\n  text\n",
    ">-\n  folded\n  text\n",
    "🎉 party",
    "\"\\ufeff bom\"",
    "yes",
    "",
    "- nested\n  - list\n",
];

fn yaml_line(rng: &mut Rng) -> String {
    let key = rng.pick(YAML_KEYS);
    let value = rng.pick(YAML_VALUES);
    let eol = if rng.one_in(5) { "\r\n" } else { "\n" };
    format!("{key}: {value}{eol}")
}

/// Plausible bodies: headings, prose, and the nasty pieces between them.
fn body(rng: &mut Rng) -> String {
    let mut out = String::new();
    for _ in 0..rng.below(8) {
        match rng.below(4) {
            0 => {
                out.push_str("## ");
                out.push_str(rng.pick(&SECTIONS));
                out.push_str(rng.pick(&["\n", "\r\n", "", "  \n"]));
            }
            1 => out.push_str(&nasty_text(rng, 6)),
            2 => out.push_str("prose line with [TUR-17]: a colon\n"),
            _ => out.push_str(rng.pick(PIECES)),
        }
    }
    out
}

/// Well-formed files to mutate: the SPEC §3.2 meeting, the SPEC §3.3
/// ticket, and CRLF / BOM spellings of both.
fn seeds() -> Vec<String> {
    let meeting = "---
id: 2026-09-01-1430-standup
title: Platform Standup
date: 2026-09-01T14:30:00+05:30
duration_sec: 2714
attendees: [Shantanu, Priya, Dev]        # from EventKit when available
calendar_event_id: \"ABC123\"              # optional
repo: ~/apps/api                          # optional link
analyzed_by: claude-code                  # agent stamps this
analyzed_at: 2026-09-01T15:32:00+05:30
---

## Summary
We talked.
### Detail
## Decisions
- Ship.

## Action Items
## Open Questions
";
    let ticket = "---
id: TICK-0001
title: Move sessions to Redis
meeting: 2026-09-01-1430-standup
status: open                  # open | in_progress | done | dropped
assignee: Shantanu
estimate: 2d
estimated_on: 2026-09-01
transcript_ref: \"00:14:22\"
synced_to: null
external_id: null
external_url: null
---

Body / acceptance notes.
";
    let mut out = Vec::new();
    for seed in [meeting, ticket] {
        out.push(seed.to_owned());
        out.push(seed.replace('\n', "\r\n"));
        out.push(format!("\u{feff}{seed}"));
    }
    out
}

/// A random char boundary in `s` (0 and `s.len()` included).
fn boundary(rng: &mut Rng, s: &str) -> usize {
    let mut i = rng.below(s.len() + 1);
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// A seed with 1-4 small edits: a piece inserted, a range deleted, a line
/// duplicated.
fn mutated(rng: &mut Rng, seeds: &[String]) -> String {
    let mut s = rng.pick(seeds).clone();
    for _ in 0..1 + rng.below(4) {
        match rng.below(3) {
            0 => {
                let at = boundary(rng, &s);
                let piece = if rng.one_in(3) {
                    yaml_line(rng)
                } else {
                    (*rng.pick(PIECES)).to_owned()
                };
                s.insert_str(at, &piece);
            }
            1 => {
                let a = boundary(rng, &s);
                let mut b = (a + 1 + rng.below(12)).min(s.len());
                while !s.is_char_boundary(b) {
                    b -= 1;
                }
                s.replace_range(a..b.max(a), "");
            }
            _ => {
                let lines: Vec<&str> = s.split_inclusive('\n').collect();
                if !lines.is_empty() {
                    let i = rng.below(lines.len());
                    let mut next = lines[..=i].concat();
                    next.push_str(lines[i]);
                    next.push_str(&lines[i + 1..].concat());
                    s = next;
                }
            }
        }
    }
    s
}

/// One generated file, in one of four shapes.
fn generated(rng: &mut Rng, seeds: &[String]) -> String {
    match rng.below(4) {
        0 => nasty_text(rng, 40),
        1 => format!("---\n{}\n---\n{}", nasty_text(rng, 12), nasty_text(rng, 20)),
        2 => {
            let bom = if rng.one_in(6) { "\u{feff}" } else { "" };
            let fence = if rng.one_in(4) { "---\r\n" } else { "---\n" };
            let yaml: String = (0..rng.below(6)).map(|_| yaml_line(rng)).collect();
            format!("{bom}{fence}{yaml}{fence}{}", body(rng))
        }
        _ => mutated(rng, seeds),
    }
}

mod folder_robustness;

/// Strings that a YAML loader might read as something other than a string.
const LOOK_ALIKES: &[&str] = &[
    "123",
    "-7",
    "+5",
    "007",
    "1_000",
    "0o17",
    "0O17",
    "0x1F",
    "0b101",
    "+.inf",
    "+.Inf",
    "+.INF",
    "-.inf",
    ".inf",
    ".nan",
    "inf",
    "NaN",
    "1e5",
    "1.5",
    ".5",
    "5.",
    "null",
    "Null",
    "NULL",
    "~",
    "true",
    "False",
    "yes",
    "no",
    "on",
    "off",
    "y",
    "n",
    "2026-09-01",
    "2026-09-01T14:30:00+05:30",
    "00:14:22",
    "1:30",
    "a: b",
    "a:b",
    "- x",
    "-",
    "--",
    "---",
    "--- x",
    "...",
    "#x",
    "a #x",
    "&a",
    "*a",
    "!!str x",
    "!x",
    "? x",
    "?",
    "|",
    ">",
    "%YAML",
    "@x",
    "`x`",
    "{a: 1}",
    "[a]",
    "'",
    "\"",
    "'quoted'",
    "\"double\"",
    "it's",
    "back\\slash",
    "\\n",
    "line\nbreak",
    "cr\ronly",
    "crlf\r\nline",
    "\n",
    "\n---\n",
    "tab\there",
    "\t",
    " leading",
    "trailing ",
    " ",
    "",
    "nul\0byte",
    "bell\u{7}",
    "esc\u{1b}",
    "del\u{7f}",
    "vt\u{b}ff\u{c}",
    "\u{85}nel",
    "ls\u{2028}ps\u{2029}",
    "\u{feff}bom",
    "mid\u{feff}bom",
    "é",
    "e\u{301}",
    "🎉",
    "👩‍👩‍👧",
    "中文",
    "\u{200b}",
    "\u{a0}nbsp\u{a0}",
];

mod never_panics;
mod path_safety;
mod resource_limits;
mod round_trip;
mod transcript_file;
mod write_safety;
