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
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "meet-ai-store-adversarial-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
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

// ---------------------------------------------------------------------------
// 1. Never panics
// ---------------------------------------------------------------------------

/// Run every parser (and every render that is allowed) over `raw`.
fn exercise_parsers(raw: &str) {
    let _ = frontmatter::split(raw);
    if let Ok(doc) = frontmatter::parse(raw) {
        let _ = frontmatter::render(&doc);
    }
    let meeting = Meeting::parse(raw);
    let _ = meeting.render();
    for heading in SECTIONS {
        let _ = meeting.section(heading);
    }
    let _ = (meeting.id(), meeting.title(), meeting.date());
    let _ = (meeting.duration_sec(), meeting.attendees());
    let ticket = Ticket::parse(raw);
    let _ = ticket.render();
    let _ = (ticket.id(), ticket.status(), ticket.transcript_ref());
    let _ = transcript::parse(raw);
    for line in raw.split('\n') {
        let _ = transcript::parse_line(line);
    }
}

#[test]
fn no_parser_panics_on_thousands_of_generated_inputs() {
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_0001);
    for case in 0..3000 {
        let raw = generated(&mut rng, &seeds);
        let result = panic::catch_unwind(AssertUnwindSafe(|| exercise_parsers(&raw)));
        assert!(result.is_ok(), "case {case} panicked on input {raw:?}");
    }
}

#[test]
fn no_parser_panics_on_any_pair_of_pieces() {
    // Exhaustive over the small space the generator samples from.
    for a in PIECES {
        for b in PIECES {
            for raw in [format!("{a}{b}"), format!("---\n{a}{b}\n---\n{b}{a}")] {
                let result = panic::catch_unwind(|| exercise_parsers(&raw));
                assert!(result.is_ok(), "panicked on input {raw:?}");
            }
        }
    }
}

#[test]
fn multibyte_characters_next_to_every_delimiter_never_split_a_char() {
    // A str slice mid-char panics. Put a multi-byte char on each side of
    // every byte the parsers look for.
    let delimiters = [
        "---", "\n", "\r\n", "\r", "## ", "[", "] ", ":", ": ", "You:", "Others:",
    ];
    let wide = ["é", "🎉", "中", "\u{feff}", "\u{2028}"];
    for d in delimiters {
        for w in wide {
            for raw in [
                format!("{w}{d}{w}"),
                format!("---\n{w}{d}{w}\n---\n{w}{d}{w}"),
                format!("[{w}0:00:01] You: {w}{d}"),
                format!("[00:00:0{w}] You:{w}"),
                format!("[00:00:01]{w} You: x"),
                format!("[00{w}00{w}01] You: x"),
                format!("[{w}] You: x"),
                format!("## {w}{d}"),
            ] {
                let result = panic::catch_unwind(|| exercise_parsers(&raw));
                assert!(result.is_ok(), "panicked on input {raw:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Round-trip stability
// ---------------------------------------------------------------------------

#[test]
fn a_meeting_that_loads_writable_is_stable_after_one_write() {
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_0002);
    let mut checked = 0;
    for case in 0..2500 {
        let raw = generated(&mut rng, &seeds);
        let first = Meeting::parse(&raw);
        if is_refused(&first.problems) {
            continue;
        }
        let text = first
            .render()
            .unwrap_or_else(|e| panic!("case {case}: render refused {raw:?}: {e}"));
        let second = Meeting::parse(&text);
        assert_eq!(
            second.frontmatter, first.frontmatter,
            "case {case}: frontmatter changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        assert_eq!(
            second.preamble, first.preamble,
            "case {case}: preamble changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        assert_eq!(
            second.sections, first.sections,
            "case {case}: sections changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        assert_eq!(
            second.problems,
            without_no_frontmatter(&first.problems),
            "case {case}: problems changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        let again = second.render().unwrap();
        assert_eq!(
            again, text,
            "case {case}: second write differs\ninput: {raw:?}"
        );
        checked += 1;
    }
    // The generator must actually reach the writable path, or this proves
    // nothing.
    assert!(checked > 1000, "only {checked} writable cases");
}

#[test]
fn a_ticket_that_loads_writable_is_stable_after_one_write() {
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_0003);
    let mut checked = 0;
    for case in 0..2500 {
        let raw = generated(&mut rng, &seeds);
        let first = Ticket::parse(&raw);
        if is_refused(&first.problems) {
            continue;
        }
        let text = first
            .render()
            .unwrap_or_else(|e| panic!("case {case}: render refused {raw:?}: {e}"));
        let second = Ticket::parse(&text);
        assert_eq!(
            second.frontmatter, first.frontmatter,
            "case {case}: frontmatter changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        assert_eq!(
            second.body, first.body,
            "case {case}: body changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        assert_eq!(
            second.problems,
            without_no_frontmatter(&first.problems),
            "case {case}: problems changed\ninput: {raw:?}\nrendered: {text:?}"
        );
        assert_eq!(
            second.render().unwrap(),
            text,
            "case {case}: second write differs\ninput: {raw:?}"
        );
        checked += 1;
    }
    assert!(checked > 1000, "only {checked} writable cases");
}

#[test]
fn a_file_with_no_frontmatter_keeps_every_byte_after_a_write() {
    // SPEC §7: writing a block on top must not lose what was there.
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_0004);
    for _ in 0..1000 {
        let raw = generated(&mut rng, &seeds);
        let meeting = Meeting::parse(&raw);
        if meeting.problems.first() == Some(&Problem::NoFrontmatter) {
            // meeting.rs documents one normalisation: `## ` heading lines are
            // rewritten trimmed, with the file's line ending. Every other
            // line survives byte for byte.
            let text = meeting.render().unwrap();
            let body = text.strip_prefix("---\n---\n").unwrap();
            let plain = |s: &str| -> Vec<String> {
                s.split_inclusive('\n')
                    .filter(|l| !l.starts_with("## "))
                    .map(str::to_owned)
                    .collect()
            };
            assert_eq!(
                plain(body),
                plain(&raw),
                "lost bytes of {raw:?} in {text:?}"
            );
        }
        let ticket = Ticket::parse(&raw);
        if ticket.problems.first() == Some(&Problem::NoFrontmatter) {
            assert_eq!(ticket.render().unwrap(), format!("---\n---\n{raw}"));
        }
    }
}

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

fn random_string(rng: &mut Rng) -> String {
    if rng.one_in(2) {
        return (*rng.pick(LOOK_ALIKES)).to_owned();
    }
    let mut out = String::new();
    for _ in 0..rng.below(4) {
        out.push_str(rng.pick(LOOK_ALIKES));
        if rng.one_in(3) {
            out.push_str(rng.pick(PIECES));
        }
    }
    out
}

/// A random YAML value, nested up to `depth` levels.
fn random_yaml(rng: &mut Rng, depth: usize) -> Yaml {
    let kinds = if depth == 0 { 5 } else { 7 };
    match rng.below(kinds) {
        0 | 1 => Yaml::String(random_string(rng)),
        2 => Yaml::Integer(rng.next_u64() as i64),
        3 => {
            Yaml::Real((*rng.pick(&["1.5", "2.50", "-0.0", "1e5", ".inf", "-.inf", ".nan"])).into())
        }
        4 => {
            if rng.one_in(2) {
                Yaml::Boolean(rng.one_in(2))
            } else {
                Yaml::Null
            }
        }
        5 => Yaml::Array(
            (0..rng.below(4))
                .map(|_| random_yaml(rng, depth - 1))
                .collect(),
        ),
        _ => {
            let mut map = yaml_rust2::yaml::Hash::new();
            for _ in 0..rng.below(4) {
                // Keys of every kind, including lists and maps.
                let key = if rng.one_in(4) {
                    random_yaml(rng, depth - 1)
                } else {
                    Yaml::String(random_string(rng))
                };
                map.insert(key, random_yaml(rng, depth - 1));
            }
            Yaml::Hash(map)
        }
    }
}

#[test]
fn every_string_set_on_a_frontmatter_reads_back_identically() {
    let mut rng = Rng::new(0x5eed_0005);
    for case in 0..600 {
        let mut fm = Frontmatter::new();
        // What each key should hold, in insertion order; `set` on an existing
        // key replaces in place.
        let mut expected: Vec<(String, String)> = Vec::new();
        for _ in 0..1 + rng.below(10) {
            let key = random_string(&mut rng);
            let value = random_string(&mut rng);
            fm.set_str(&key, Some(&value));
            match expected.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => expected.push((key, value)),
            }
        }
        let body = nasty_text(&mut rng, 8);
        let doc = Document {
            frontmatter: fm.clone(),
            body: body.clone(),
        };
        let text = frontmatter::render(&doc);
        let back = frontmatter::parse(&text)
            .unwrap_or_else(|p| panic!("case {case}: {p:?}\nrendered: {text:?}"));
        assert_eq!(back.body, body, "case {case}: body\nrendered: {text:?}");
        assert_eq!(
            back.frontmatter, fm,
            "case {case}: frontmatter\nrendered: {text:?}"
        );
        let keys: Vec<_> = expected.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(back.frontmatter.keys(), keys, "case {case}: key order");
        for (key, value) in &expected {
            assert_eq!(
                back.frontmatter.get_str(key).as_deref(),
                Some(value.as_str()),
                "case {case}: {key:?}\nrendered: {text:?}"
            );
        }
        // And the same through a ticket, which is how the app writes one.
        let mut ticket = Ticket::new("TICK-0001", "T", "m1");
        for (key, value) in &expected {
            ticket.frontmatter.set_str(key, Some(value));
        }
        let text = ticket.render().unwrap();
        let again = Ticket::parse(&text);
        assert!(!is_refused(&again.problems), "case {case}: {text:?}");
        assert_eq!(
            again.frontmatter, ticket.frontmatter,
            "case {case}: {text:?}"
        );
    }
}

#[test]
fn nested_values_and_odd_keys_set_on_a_frontmatter_round_trip() {
    let mut rng = Rng::new(0x5eed_0006);
    for case in 0..600 {
        let mut fm = Frontmatter::new();
        // Half the cases force the all-quoted fallback emitter: `0o17` is a
        // string `YamlEmitter` leaves bare.
        if rng.one_in(2) {
            fm.set_str("force", Some("0o17"));
        }
        for _ in 0..1 + rng.below(6) {
            let key = random_string(&mut rng);
            fm.set(&key, random_yaml(&mut rng, 3));
        }
        let doc = Document {
            frontmatter: fm.clone(),
            body: "body\n".into(),
        };
        let text = frontmatter::render(&doc);
        let back = frontmatter::parse(&text)
            .unwrap_or_else(|p| panic!("case {case}: {p:?}\nrendered: {text:?}"));
        assert_eq!(back, doc, "case {case}\nrendered: {text:?}");
        assert_eq!(
            frontmatter::render(&back),
            text,
            "case {case}: second render differs"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Transcript
// ---------------------------------------------------------------------------

const SPEECH: &[&str] = &[
    "hello",
    "]",
    "[",
    ":",
    ": ",
    "] ",
    "[00:00:01] You:",
    "[00:00:01] Others: nested",
    "You:",
    "Others:",
    "\t",
    "\n",
    "\r\n",
    "\r",
    "  ",
    " ",
    "é",
    "🎉",
    "中文",
    "\u{2028}",
    "\u{85}",
    "\u{a0}",
    "\u{200b}",
    "\u{feff}",
    "\u{3000}",
    "#",
    "## Summary",
    "---",
    "\\",
    "\"",
    "'",
    "\0",
];

fn random_speech(rng: &mut Rng) -> String {
    (0..rng.below(12)).map(|_| *rng.pick(SPEECH)).collect()
}

fn random_speaker(rng: &mut Rng) -> Speaker {
    if rng.one_in(2) {
        Speaker::You
    } else {
        Speaker::Others
    }
}

#[test]
fn every_formatted_line_parses_back_to_what_was_said() {
    let mut rng = Rng::new(0x5eed_0007);
    for _ in 0..5000 {
        // `{h:02}` grows to three digits past 99h, which §3.4's regex then
        // rejects — a documented limitation (see transcript.rs), so stay
        // under it.
        let start_sec = rng.below(360_000) as u64;
        let speaker = random_speaker(&mut rng);
        let text = random_speech(&mut rng);
        let want = collapsed(&text);
        let Some(line) = transcript::format_line(start_sec, speaker, &text) else {
            assert_eq!(want, "", "{text:?} was dropped");
            continue;
        };
        assert!(!line.contains('\n') && !line.contains('\r'), "{line:?}");
        let (time, sec, who, said) =
            transcript::parse_line(&line).unwrap_or_else(|| panic!("{line:?} did not parse back"));
        assert_eq!(sec, start_sec, "{line:?}");
        assert_eq!(
            time,
            format!(
                "{:02}:{:02}:{:02}",
                start_sec / 3600,
                start_sec % 3600 / 60,
                start_sec % 60
            )
        );
        assert_eq!(who, speaker, "{line:?}");
        assert_eq!(said, want, "{line:?}");
    }
}

/// Have stt's sink append `count` random lines to `path` and check they all
/// come back, in order, after whatever was there, which must be untouched.
///
/// The sink is SPEC §3.4's only writer of `transcript.md`, so this is the
/// write path the reader actually has to keep up with. Text goes through
/// `stt::collapse_whitespace` first, as the engines do before handing it over.
fn append_and_check(rng: &mut Rng, path: &Path, count: usize) {
    use stt::TranscriptSink;

    let before = fs::read(path).unwrap_or_default();
    let mut appended = Vec::new();
    let mut sink = stt::MarkdownSink::create(path).unwrap();
    for _ in 0..count {
        let start_sec = rng.below(360_000) as u64;
        let speaker = random_speaker(rng);
        let text = random_speech(rng);
        let Some(text) = stt::collapse_whitespace(&text) else {
            continue;
        };
        sink.write(&stt::Utterance {
            start_sec,
            speaker,
            text: text.clone(),
        })
        .unwrap();
        appended.push((start_sec, speaker, text));
    }
    sink.flush().unwrap();
    drop(sink);
    let after = fs::read(path).unwrap();
    assert!(
        after.starts_with(&before),
        "earlier content changed:\nbefore {:?}\nafter  {:?}",
        String::from_utf8_lossy(&before),
        String::from_utf8_lossy(&after)
    );
    let t = transcript::read(path).unwrap().unwrap();
    let tail: Vec<_> = t.lines[t.lines.len() - appended.len()..]
        .iter()
        .map(|l| (l.start_sec, l.speaker, l.text.clone()))
        .collect();
    assert_eq!(tail, appended);
    assert!(after.ends_with(b"\n"));
}

#[test]
fn lines_the_stt_sink_appends_come_back_in_order_after_whatever_was_there() {
    // Every start ends in a newline (or is empty): the sink appends without
    // adding a separator, so a hand edit that drops the final newline glues
    // the next line onto it. That is the writer's concern, not the reader's.
    let scratch = Scratch::new("append");
    let mut rng = Rng::new(0x5eed_0008);
    let starts = [
        ("missing", None),
        ("empty", Some("")),
        (
            "crlf",
            Some("[00:00:04] You: hi\r\n[00:00:05] Others: hey\r\n"),
        ),
        ("junk", Some("# Transcript\n\nnot a line\n[bad] You: x\n")),
        ("multibyte-last", Some("[00:00:04] You: 🎉\n")),
        ("only-newlines", Some("\n\n\n")),
    ];
    for (name, start) in starts {
        let path = scratch.path().join(name).join(TRANSCRIPT_FILE);
        if let Some(start) = start {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, start).unwrap();
        }
        append_and_check(&mut rng, &path, 200);
        // A second recording session opens the file again and carries on.
        append_and_check(&mut rng, &path, 50);
    }
}

#[test]
fn every_parsed_line_counts_every_line_before_it() {
    // `seq` is the 0-based line number, skipped lines included — for CRLF,
    // LF and a mix of both.
    let mut rng = Rng::new(0x5eed_0009);
    for _ in 0..300 {
        let mut raw = String::new();
        let mut expected = Vec::new();
        for i in 0..rng.below(30) {
            if rng.one_in(3) {
                raw.push_str(rng.pick(&["junk", "", "   ", "[00:00:01]You: x", "## h"]));
            } else {
                let sec = rng.below(360_000) as u64;
                raw.push_str(&transcript::format_line(sec, Speaker::You, "x").unwrap());
                expected.push((i, sec));
            }
            raw.push_str(rng.pick(&["\n", "\r\n"]));
        }
        let t = transcript::parse(&raw);
        let got: Vec<_> = t.lines.iter().map(|l| (l.seq, l.start_sec)).collect();
        assert_eq!(got, expected, "{raw:?}");
    }
}

#[test]
fn a_bom_at_the_start_of_a_transcript_does_not_hide_the_first_line() {
    // frontmatter::split tolerates a BOM; transcript::parse does not, so a
    // transcript re-saved by a Windows editor reports its first utterance
    // as unparsed.
    let t = transcript::parse("\u{feff}[00:00:04] You: hi\n[00:00:05] Others: hey\n");
    assert_eq!(t.problems, vec![], "{t:?}");
    assert_eq!(t.lines.len(), 2, "{t:?}");
}

// ---------------------------------------------------------------------------
// 4. Folder robustness
// ---------------------------------------------------------------------------

#[test]
fn scan_returns_every_folder_of_a_hostile_tree_and_names_each_broken_file() {
    let scratch = Scratch::new("hostile-tree");
    let root = scratch.path();
    let bad_utf8: &[u8] = b"---\nid: m\n---\ncaf\xe9 \xff\xfe\n";
    let unicode_id = "2026-01-06-0900-会議-☕-é";

    let dir = |id: &str| {
        let d = root.join(id);
        fs::create_dir_all(&d).unwrap();
        d
    };

    let d = dir("2026-01-01-0900-meeting-md-is-a-folder");
    fs::create_dir_all(d.join(MEETING_FILE)).unwrap();

    let d = dir("2026-01-02-0900-tickets-is-a-file");
    fs::write(d.join(TICKETS_DIR), "not a folder").unwrap();

    let d = dir("2026-01-03-0900-empty-ticket");
    fs::create_dir_all(d.join(TICKETS_DIR)).unwrap();
    fs::write(d.join(TICKETS_DIR).join("TICK-0001.md"), "").unwrap();

    let d = dir("2026-01-04-0900-not-utf8");
    fs::create_dir_all(d.join(TICKETS_DIR)).unwrap();
    fs::write(d.join(MEETING_FILE), bad_utf8).unwrap();
    fs::write(d.join(TRANSCRIPT_FILE), b"[00:00:04] You: \xc3\x28\n").unwrap();
    fs::write(d.join(NOTES_FILE), b"caf\xe9 notes").unwrap();
    fs::write(d.join(TICKETS_DIR).join("TICK-0002.md"), bad_utf8).unwrap();

    let d = dir("2026-01-05-0900-notes-and-transcript-are-folders");
    fs::create_dir_all(d.join(NOTES_FILE)).unwrap();
    fs::create_dir_all(d.join(TRANSCRIPT_FILE)).unwrap();

    let d = dir(unicode_id);
    Meeting::new(unicode_id, "☕ 会議")
        .write(&d.join(MEETING_FILE))
        .unwrap();

    dir("2026-01-07-0900-empty");

    // Not meetings: a dot-folder and a plain file.
    fs::create_dir_all(root.join(".hidden").join(TICKETS_DIR)).unwrap();
    fs::write(root.join("2026-01-08-0900-a-file.md"), "---\n---\n").unwrap();

    let folders = folder::scan(root).expect("a hostile tree still scans");
    let ids: Vec<_> = folders.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "2026-01-07-0900-empty",
            unicode_id,
            "2026-01-05-0900-notes-and-transcript-are-folders",
            "2026-01-04-0900-not-utf8",
            "2026-01-03-0900-empty-ticket",
            "2026-01-02-0900-tickets-is-a-file",
            "2026-01-01-0900-meeting-md-is-a-folder",
        ]
    );
    let by_id = |id: &str| folders.iter().find(|f| f.id == id).unwrap();

    let f = by_id("2026-01-01-0900-meeting-md-is-a-folder");
    assert_eq!(labelled(f), pairs(&[("meeting.md", "Unreadable")]));
    assert!(f.meeting.is_none());

    let f = by_id("2026-01-02-0900-tickets-is-a-file");
    assert_eq!(labelled(f), pairs(&[("tickets", "Unreadable")]));
    assert!(f.tickets.is_empty());

    let f = by_id("2026-01-03-0900-empty-ticket");
    assert_eq!(
        labelled(f),
        pairs(&[
            ("tickets/TICK-0001.md", "NoFrontmatter"),
            ("tickets/TICK-0001.md", "MissingField(id)"),
            ("tickets/TICK-0001.md", "MissingField(title)"),
        ])
    );
    assert_eq!(f.tickets.len(), 1, "an empty ticket still loads");

    let f = by_id("2026-01-04-0900-not-utf8");
    assert_eq!(
        labelled(f),
        pairs(&[
            ("meeting.md", "Unreadable"),
            ("transcript.md", "Unreadable"),
            ("notes.md", "Unreadable"),
            ("tickets/TICK-0002.md", "Unreadable"),
        ])
    );
    // A lossy decode here would hand the notes pane U+FFFD, and its next
    // autosave would replace the real bytes for good.
    assert_eq!(f.notes, "");
    assert!(f.meeting.is_some() && f.transcript.is_some());
    assert_eq!(f.tickets.len(), 1);

    let f = by_id("2026-01-05-0900-notes-and-transcript-are-folders");
    assert_eq!(
        labelled(f),
        pairs(&[("transcript.md", "Unreadable"), ("notes.md", "Unreadable")])
    );

    let f = by_id(unicode_id);
    assert!(!f.needs_attention(), "{:?}", f.problems);
    assert_eq!(
        f.meeting.as_ref().unwrap().title().as_deref(),
        Some("☕ 会議")
    );

    let f = by_id("2026-01-07-0900-empty");
    assert!(!f.needs_attention(), "{:?}", f.problems);

    // The same tree, one folder at a time, agrees with the scan.
    for f in &folders {
        assert_eq!(&folder::load(&f.path).unwrap(), f, "{}", f.id);
    }
}

// ---------------------------------------------------------------------------
// 5. Write safety
// ---------------------------------------------------------------------------

const BROKEN_YAML: &[&str] = &[
    "---\nid: m1\ntitle: [unclosed\n---\n## Summary\nkeep\n",
    "---\nid: m1\nid: m2\n---\n",
    "---\n- a\n- b\n---\n",
    "---\njust a scalar\n---\n",
    "---\na: 1\n...\nb: 2\n---\n",
    "---\na: 1\n--- \nb: 2\n---\n",
    "---\nkey: \"open quote\n---\n",
    "---\nkey: 'open\n---\n",
    "---\na: *undefined\n---\n",
    "\u{feff}---\r\nid: m1\r\ntitle: {a: 1\r\n---\r\nbody\r\n",
    "---\nid: m1\n  title: bad indent\n---\n",
    "---\n{a: 1}: [\n---\n",
];

/// Invalid UTF-8 of every kind: a stray continuation byte, a truncated
/// sequence at the end, an overlong encoding, an encoded surrogate, 0xFF.
const BAD_BYTES: &[&[u8]] = &[
    b"---\nid: m1\ntitle: T\n---\n\x80",
    b"---\nid: m1\ntitle: T\n---\nend \xe2\x82",
    b"---\nid: m1\ntitle: T\n---\n\xc0\xaf",
    b"---\nid: m1\ntitle: T\n---\n\xed\xa0\x80",
    b"\xff\xfe---\n",
    b"---\nid: \xf0\x9f\x8e\n---\n",
];

#[test]
fn a_file_that_loaded_broken_is_never_overwritten() {
    let scratch = Scratch::new("refuse");
    let dir = scratch.path();
    let meeting_path = dir.join(MEETING_FILE);
    let ticket_path = dir.join("TICK-0001.md");
    let mut inputs: Vec<Vec<u8>> = BROKEN_YAML.iter().map(|s| s.as_bytes().to_vec()).collect();
    inputs.extend(BAD_BYTES.iter().map(|b| b.to_vec()));

    for bytes in &inputs {
        let shown = String::from_utf8_lossy(bytes);
        fs::write(&meeting_path, bytes).unwrap();
        fs::write(&ticket_path, bytes).unwrap();

        let meeting = Meeting::read(&meeting_path).unwrap().unwrap();
        assert!(
            is_refused(&meeting.problems),
            "{shown:?}: {:?}",
            meeting.problems
        );
        assert!(meeting.render().is_err(), "{shown:?}");
        assert!(
            matches!(
                meeting.write(&meeting_path),
                Err(Error::Frontmatter { .. } | Error::Io(_))
            ),
            "{shown:?}"
        );
        // Even after a caller edits it, it must stay refused.
        let mut edited = meeting.clone();
        edited.set_section("Summary", "new text");
        edited.frontmatter.set_str("id", Some("m1"));
        assert!(edited.write(&meeting_path).is_err(), "{shown:?}");

        let ticket = Ticket::read(&ticket_path).unwrap();
        assert!(
            is_refused(&ticket.problems),
            "{shown:?}: {:?}",
            ticket.problems
        );
        assert!(ticket.render().is_err(), "{shown:?}");
        let mut edited = ticket.clone();
        edited.set_status(store::ticket::Status::Done);
        assert!(edited.write(&ticket_path).is_err(), "{shown:?}");

        assert_eq!(&fs::read(&meeting_path).unwrap(), bytes, "{shown:?}");
        assert_eq!(&fs::read(&ticket_path).unwrap(), bytes, "{shown:?}");
        assert_eq!(dotfiles(dir), Vec::<String>::new(), "{shown:?}");
    }
}

#[test]
fn notes_with_everything_nasty_in_them_round_trip_exactly() {
    let scratch = Scratch::new("notes");
    let dir = scratch.path();
    let mut rng = Rng::new(0x5eed_000a);
    let mut bodies: Vec<String> = vec![
        String::new(),
        "\u{feff}".into(),
        "\0".into(),
        "\r".into(),
        "\r\n".into(),
        "no newline".into(),
        "---\nid: not frontmatter\n---\n## Summary\n".into(),
        PIECES.concat(),
        LOOK_ALIKES.concat(),
    ];
    bodies.extend((0..200).map(|_| nasty_text(&mut rng, 30)));
    for body in &bodies {
        notes::write(dir, body).unwrap();
        assert_eq!(&notes::read(dir).unwrap(), body);
        assert_eq!(fs::read(dir.join(NOTES_FILE)).unwrap(), body.as_bytes());
    }
    assert_eq!(dotfiles(dir), Vec::<String>::new());
}

#[test]
fn notes_that_are_not_utf8_are_never_overwritten() {
    let scratch = Scratch::new("notes-bad-bytes");
    let dir = scratch.path();
    let path = dir.join(NOTES_FILE);
    let invalid_data = |result: Result<(), Error>| matches!(result, Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::InvalidData);
    for bytes in BAD_BYTES {
        let shown = String::from_utf8_lossy(bytes);
        fs::write(&path, bytes).unwrap();
        assert!(invalid_data(notes::read(dir).map(drop)), "{shown:?}");
        assert!(invalid_data(notes::write(dir, "autosave")), "{shown:?}");
        assert!(invalid_data(notes::write(dir, "")), "{shown:?}");
        assert_eq!(fs::read(&path).unwrap(), *bytes, "{shown:?}");
        assert_eq!(dotfiles(dir), Vec::<String>::new(), "{shown:?}");
    }
}

#[test]
fn successful_writes_never_leave_a_temp_file_behind() {
    let scratch = Scratch::new("no-temp");
    let seeds = seeds();
    let mut rng = Rng::new(0x5eed_000b);
    let dir = scratch.path().join("2026-09-01-1430-standup");
    let tickets = dir.join(TICKETS_DIR);
    let mut writes = 0;
    for i in 0..300 {
        let raw = generated(&mut rng, &seeds);
        let meeting = Meeting::parse(&raw);
        if !is_refused(&meeting.problems) {
            let path = dir.join(MEETING_FILE);
            meeting.write(&path).unwrap();
            let back = Meeting::read(&path).unwrap().unwrap();
            assert_eq!(back.sections, meeting.sections);
            writes += 1;
        }
        let ticket = Ticket::parse(&raw);
        if !is_refused(&ticket.problems) {
            let path = tickets.join(format!("TICK-{:04}.md", i % 7 + 1));
            ticket.write(&path).unwrap();
            assert_eq!(Ticket::read(&path).unwrap().body, ticket.body);
            writes += 1;
        }
        notes::write(&dir, &raw).unwrap();
    }
    assert!(writes > 200, "only {writes} writes");
    assert_eq!(dotfiles(&dir), Vec::<String>::new());
    assert_eq!(dotfiles(&tickets), Vec::<String>::new());
    assert_eq!(folder::load(&dir).unwrap().tickets.len(), 7);
}

#[test]
fn a_failed_write_leaves_no_temp_file_behind() {
    // `write_atomic` writes a dotfile temp (`.notes.md.tmp.<pid>.<n>`), then
    // renames it over the target. When the rename fails (here: the target is
    // a non-empty folder) the temp file must be cleaned up, not left behind.
    let scratch = Scratch::new("failed-write");
    let dir = scratch.path();
    fs::create_dir_all(dir.join(NOTES_FILE).join("child")).unwrap();
    // `notes::write` now refuses before it gets this far (the folder cannot
    // be read as notes), so the rename failure is driven directly.
    assert!(notes::write(dir, "draft").is_err());
    assert!(store::write_atomic(&dir.join(NOTES_FILE), "draft").is_err());
    fs::create_dir_all(dir.join(MEETING_FILE).join("child")).unwrap();
    assert!(
        Meeting::new("m1", "T")
            .write(&dir.join(MEETING_FILE))
            .is_err()
    );
    assert_eq!(dotfiles(dir), Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// 6. Path safety
// ---------------------------------------------------------------------------

#[test]
fn an_id_that_is_not_a_plain_folder_name_is_refused() {
    let root = Path::new("meetings");
    for hostile in [
        "..",
        ".",
        "a/..",
        "../x",
        "a/../..",
        "./a",
        "a/",
        "/a",
        "/",
        "/etc/passwd",
        "C:\\x",
        "C:\\",
        "\\\\server\\share",
        "\\\\?\\C:\\x",
        "a\\..",
        "..\\..",
        "",
        ".app",
        ".hidden",
        "..hidden",
        ".🎉",
    ] {
        assert!(
            matches!(folder::meeting_dir(root, hostile), Err(Error::BadId(id)) if id == hostile),
            "{hostile:?} must be refused"
        );
    }
}

#[test]
fn a_plain_id_resolves_to_a_folder_directly_under_the_root() {
    let root = Path::new("meetings");
    for id in [
        "2026-09-01-1430-standup",
        "TICK-0001",
        "会議",
        "☕",
        "é",
        "e\u{301}",
        "a b",
        "a.b",
        "x..y",
        "~",
        "-",
        "a..",
    ] {
        assert_eq!(
            folder::meeting_dir(root, id).unwrap(),
            root.join(id),
            "{id:?}"
        );
    }
}

#[test]
fn whatever_id_is_accepted_never_leaves_the_root() {
    // Generated ids from path-ish pieces. Every accepted one must name a
    // direct child of the root — on every OS, which is what would catch a
    // Windows drive prefix like `C:` being joined as an absolute path.
    let pieces = [
        "a", ".", "..", "/", "\\", ":", "C:", "c:", "~", " ", "\0", "é", "🎉", "?", "*", "NUL",
        "CON",
    ];
    let root = Path::new("meetings");
    let mut rng = Rng::new(0x5eed_000c);
    let mut ids: Vec<String> = ["C:", "c:", "C:x", "NUL", "CON", "COM1", "~root"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    ids.extend((0..3000).map(|_| {
        (0..1 + rng.below(5))
            .map(|_| *rng.pick(&pieces))
            .collect::<String>()
    }));
    for id in ids {
        if let Ok(path) = folder::meeting_dir(root, &id) {
            assert_eq!(path.parent(), Some(root), "{id:?} escaped to {path:?}");
            assert!(!id.starts_with('.'), "{id:?}");
            assert!(!id.contains('/') && !id.contains('\\'), "{id:?}");
        }
    }
}

#[test]
fn an_id_containing_nul_is_refused() {
    // No OS allows NUL in a file name, so such an id can only fail later
    // with a confusing I/O error instead of `BadId` up front.
    let root = Path::new("meetings");
    for id in ["\0", "a\0b", "2026-09-01-1430-standup\0"] {
        assert!(
            matches!(folder::meeting_dir(root, id), Err(Error::BadId(_))),
            "{id:?} must be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// Resource limits
// ---------------------------------------------------------------------------

/// Env var that makes a test run its dangerous half in a child process.
const CHILD: &str = "MEET_AI_STORE_ADVERSARIAL_CHILD";

/// Run the named test again in a child copy of this test binary, with
/// [`CHILD`] set. A stack overflow aborts the whole process and cannot be
/// caught, so this is the only way to report one as a plain failure instead
/// of taking every other test down with it.
fn run_in_a_child(test: &str) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--ignored", "--test-threads=1"])
        .env(CHILD, "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
}

#[test]
fn deeply_nested_frontmatter_loads_flagged_instead_of_aborting_the_process() {
    // ~40 KB: `k:` followed by 20 000 levels of nesting. yaml-rust2's
    // loader recurses once per level; about 1 000 levels (2 KB) is already
    // enough on a 2 MB test thread in a debug build.
    if std::env::var_os(CHILD).is_some() {
        // Two spellings of the same nesting: compact block lists, and
        // compact mappings with empty keys (`: : : x`).
        for unit in ["- ", ": "] {
            let raw = format!("---\nk:\n  {}x\n---\n## Summary\n", unit.repeat(20_000));
            let meeting = Meeting::parse(&raw);
            assert!(meeting.section("Summary").is_some());
            let _ = Ticket::parse(&raw);
        }
        return;
    }
    let status =
        run_in_a_child("deeply_nested_frontmatter_loads_flagged_instead_of_aborting_the_process");
    assert!(
        status.success(),
        "parsing the file killed the process: {status}"
    );
}

#[test]
fn a_small_file_of_nested_aliases_is_not_expanded_into_megabytes() {
    // Five levels of ten aliases each: about 300 bytes of frontmatter, 10^5
    // nodes once expanded. Seven levels would be 10^7 and exhaust memory.
    let mut raw = String::from("---\nid: m1\ntitle: T\na0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for i in 1..5 {
        let refs = vec![format!("*a{}", i - 1); 10].join(", ");
        raw.push_str(&format!("a{i}: &a{i} [{refs}]\n"));
    }
    raw.push_str("---\n");
    let meeting = Meeting::parse(&raw);
    if is_refused(&meeting.problems) {
        return; // Refusing the file is a fine answer.
    }
    let rendered = meeting.render().unwrap();
    assert!(
        rendered.len() < 64 * 1024,
        "{} bytes in, {} bytes out",
        raw.len(),
        rendered.len()
    );
}

#[test]
fn flow_nesting_up_to_the_loaders_own_limit_does_not_crash() {
    // yaml-rust2 caps flow nesting (`[[[...`) at 255 levels itself; make
    // sure everything store does with such a value fits a test thread's
    // stack.
    for depth in [100, 254, 255, 256, 1000] {
        let raw = format!("---\nk: {}x{}\n---\n", "[".repeat(depth), "]".repeat(depth));
        let meeting = Meeting::parse(&raw);
        if let Ok(text) = meeting.render() {
            assert_eq!(Meeting::parse(&text).frontmatter, meeting.frontmatter);
        }
        let _ = Ticket::parse(&raw).render();
    }
}

#[test]
fn a_very_long_line_parses_in_reasonable_time() {
    let start = std::time::Instant::now();
    let long = "é🎉 [x]: ".repeat(100_000);
    let raw = format!("---\nid: m1\ntitle: \"{long}\"\n---\n## Summary\n{long}\n");
    let meeting = Meeting::parse(&raw);
    assert_eq!(meeting.title().as_deref(), Some(long.as_str()));
    let text = meeting.render().unwrap();
    assert_eq!(Meeting::parse(&text).sections, meeting.sections);
    let line = transcript::format_line(1, Speaker::You, &long).unwrap();
    assert!(transcript::parse_line(&line).is_some());
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}
