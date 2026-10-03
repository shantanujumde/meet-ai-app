//! The video-call link in a calendar event (TUR-77).
//!
//! Calendar.app has no "meeting link" field: Zoom, Meet and Teams invites put
//! the link in the event's URL, its location or its notes, wherever the
//! inviting calendar chose. [`extract_join_url`] looks in that order and
//! returns the first link to a known video service, so the menu bar's
//! **Join** opens the call and never a random web page from the notes.
//!
//! Pure string work, no OS code: EventKit (`eventkit.rs`) hands it the three
//! fields, and the cloud providers have a link field of their own.

/// A known video-call link: a host (or any subdomain of it, so
/// `acme.zoom.us` matches `zoom.us`) and the path it must start with. The
/// path must go on past the prefix: `meet.google.com/` alone is the landing
/// page, not a call.
#[derive(Debug, Clone, Copy)]
struct Pattern {
    host: &'static str,
    path: &'static str,
}

/// Zoom meetings, personal rooms and webinars.
const ZOOM: &[Pattern] = &[
    Pattern {
        host: "zoom.us",
        path: "/j/",
    },
    Pattern {
        host: "zoom.us",
        path: "/my/",
    },
    Pattern {
        host: "zoom.us",
        path: "/w/",
    },
];

/// Google Meet: `meet.google.com/abc-defg-hij`.
const GOOGLE_MEET: &[Pattern] = &[Pattern {
    host: "meet.google.com",
    path: "/",
}];

/// Microsoft Teams, work and personal accounts.
const TEAMS: &[Pattern] = &[
    Pattern {
        host: "teams.microsoft.com",
        path: "/l/meetup-join/",
    },
    Pattern {
        host: "teams.live.com",
        path: "/meet/",
    },
];

/// Webex: every meeting link is on a `<site>.webex.com` host.
const WEBEX: &[Pattern] = &[Pattern {
    host: "webex.com",
    path: "/",
}];

/// Slack huddles: `app.slack.com/huddle/T…/C…`.
const SLACK_HUDDLE: &[Pattern] = &[Pattern {
    host: "slack.com",
    path: "/huddle/",
}];

/// Every service, in the order a link is checked against them.
const SERVICES: &[&[Pattern]] = &[ZOOM, GOOGLE_MEET, TEAMS, WEBEX, SLACK_HUDDLE];

/// Characters that end a link in free text: whitespace is handled apart,
/// these are the brackets and quotes notes wrap links in
/// (`<https://…>`, `href="https://…"`, `(https://…)`).
const DELIMITERS: &[char] = &['<', '>', '"', '\'', '(', ')', '[', ']', '{', '}', '|', '`'];

/// Punctuation a sentence leaves on the end of a link ("join at
/// https://zoom.us/j/1.") that is never part of it.
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', '*', '\\'];

/// The link to join the call, from the first of `url`, `location` and
/// `notes` that holds one, or `None` when none does.
///
/// Only links to a known video service count (see [`SERVICES`]). Within one
/// field the first such link wins. A link written without a scheme
/// (`zoom.us/j/123`) is returned with `https://` in front, and trailing
/// sentence punctuation is trimmed off.
pub fn extract_join_url(
    url: Option<&str>,
    location: Option<&str>,
    notes: Option<&str>,
) -> Option<String> {
    [url, location, notes]
        .into_iter()
        .flatten()
        .find_map(first_link_in)
}

/// The first video-call link in `text`.
fn first_link_in(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || DELIMITERS.contains(&c))
        .filter_map(candidate)
        .find(|link| is_video_link(link))
}

/// `token` as a full `https://` link, if it looks like one at all.
fn candidate(token: &str) -> Option<String> {
    // HTML notes (Google, Outlook) escape the `&` between query parameters.
    let token = token.trim_end_matches(TRAILING).replace("&amp;", "&");
    if token.is_empty() {
        return None;
    }
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(token);
    }
    // No scheme: a bare `zoom.us/j/1` in a location field. Anything else with
    // a scheme of its own (`mailto:`, `tel:`) is not a link to a call.
    (!lower.contains("://") && !lower.contains(':')).then(|| format!("https://{token}"))
}

/// Whether `link` (with a scheme) points at a known video service.
fn is_video_link(link: &str) -> bool {
    let Some((host, path)) = host_and_path(link) else {
        return false;
    };
    SERVICES.iter().flat_map(|service| service.iter()).any(|p| {
        host_matches(&host, p.host)
            && path.len() > p.path.len()
            && path.to_ascii_lowercase().starts_with(p.path)
    })
}

/// The lower-cased host and the path of `link`, without user info, port,
/// query or fragment.
fn host_and_path(link: &str) -> Option<(String, &str)> {
    let (_, rest) = link.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let after = &rest[end..];
    let path_end = after.find(['?', '#']).unwrap_or(after.len());
    Some((host, &after[..path_end]))
}

/// `host` is `want` or a subdomain of it: `acme.zoom.us` matches `zoom.us`,
/// `notzoom.us` does not.
fn host_matches(host: &str, want: &str) -> bool {
    host == want
        || host
            .strip_suffix(want)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(text: &str) -> Option<String> {
        extract_join_url(None, None, Some(text))
    }

    #[test]
    fn zoom_from_the_url_field() {
        assert_eq!(
            extract_join_url(Some("https://acme.zoom.us/j/123456789?pwd=abc"), None, None)
                .as_deref(),
            Some("https://acme.zoom.us/j/123456789?pwd=abc")
        );
    }

    #[test]
    fn google_meet_from_the_location() {
        assert_eq!(
            extract_join_url(None, Some("https://meet.google.com/abc-defg-hij"), None).as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
    }

    #[test]
    fn teams_from_html_notes() {
        let body = r#"<a href="https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%22Tid%22%7d&amp;x=1">Join</a>"#;
        assert_eq!(
            notes(body).as_deref(),
            Some(
                "https://teams.microsoft.com/l/meetup-join/19%3ameeting_abc%40thread.v2/0?context=%7b%22Tid%22%7d&x=1"
            )
        );
    }

    #[test]
    fn link_in_the_notes_after_other_links() {
        let body = "Agenda: https://docs.google.com/document/d/1\n\
                    Join Zoom Meeting\n\
                    https://zoom.us/j/987654321.\n\
                    Dial in: tel:+15551234567";
        assert_eq!(notes(body).as_deref(), Some("https://zoom.us/j/987654321"));
    }

    #[test]
    fn webex_and_slack_huddles() {
        assert_eq!(
            notes("Join: https://acme.webex.com/meet/priya").as_deref(),
            Some("https://acme.webex.com/meet/priya")
        );
        assert_eq!(
            notes("(https://app.slack.com/huddle/T0123/C0456)").as_deref(),
            Some("https://app.slack.com/huddle/T0123/C0456")
        );
    }

    #[test]
    fn none_when_there_is_no_call_link() {
        assert_eq!(extract_join_url(None, None, None), None);
        assert_eq!(
            extract_join_url(
                Some("https://example.com/event"),
                Some("Room 4B"),
                Some("Bring snacks. https://zoom.us/ and https://meet.google.com/"),
            ),
            None
        );
        // A look-alike host is not the service.
        assert_eq!(notes("https://notzoom.us/j/1"), None);
        assert_eq!(notes("mailto:priya@zoom.us"), None);
    }

    #[test]
    fn url_wins_over_location_wins_over_notes() {
        let url = "https://zoom.us/j/1";
        let location = "https://meet.google.com/aaa-bbbb-ccc";
        let body = "https://teams.microsoft.com/l/meetup-join/x";
        assert_eq!(
            extract_join_url(Some(url), Some(location), Some(body)).as_deref(),
            Some(url)
        );
        assert_eq!(
            extract_join_url(Some("https://example.com"), Some(location), Some(body)).as_deref(),
            Some(location)
        );
        assert_eq!(
            extract_join_url(None, Some("Room 4B"), Some(body)).as_deref(),
            Some(body)
        );
    }

    #[test]
    fn a_bare_link_gets_a_scheme_and_loses_its_punctuation() {
        assert_eq!(
            extract_join_url(None, Some("zoom.us/j/42;"), None).as_deref(),
            Some("https://zoom.us/j/42")
        );
        assert_eq!(
            notes("<https://meet.google.com/xyz-abcd-efg>,").as_deref(),
            Some("https://meet.google.com/xyz-abcd-efg")
        );
    }

    #[test]
    fn hosts_and_paths_are_matched_without_case_ports_or_user_info() {
        assert_eq!(
            notes("HTTPS://Acme.Zoom.US:443/J/5").as_deref(),
            Some("HTTPS://Acme.Zoom.US:443/J/5")
        );
        assert!(is_video_link("https://user@meet.google.com/abc"));
        assert!(!is_video_link("https://meet.google.com/?authuser=0"));
    }
}
