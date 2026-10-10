//! 6. Path safety.

use super::*;

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
        // TUR-158: Windows devices and names Windows trims.
        "a..",
        "a.",
        "a ",
        "NUL",
        "con",
        "Com1.md",
        "lpt9",
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
