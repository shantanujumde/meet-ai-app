//! 2. Round-trip stability.

use super::*;

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
