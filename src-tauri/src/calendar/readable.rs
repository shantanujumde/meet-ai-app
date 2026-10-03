//! Which calendars a read asks (TUR-88): one answer for the Today pane, the
//! menu bar and the meeting reminders, so the three cannot drift apart again.
//!
//! The rule: read every configured calendar that can be read now. Only the
//! Today pane may show the macOS calendar prompt, because the user is looking
//! at it and can see why. The menu bar and the reminders run in the
//! background, so until that prompt has been answered they leave Calendar.app
//! out and still read the Google and Microsoft sign-ins. A Mac whose only
//! calendars are cloud ones never shows the prompt at all, and still gets its
//! reminders.

use super::SharedProvider;
use crate::config::Provider;

/// Whether a read may show the macOS calendar prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// The Today pane: Calendar.app may ask for access.
    Allowed,
    /// The menu bar and the reminders: never ask from the background.
    Never,
}

/// Calendar.app is configured and its permission prompt has not been
/// answered yet, so reading it would show the prompt. `eventkit_answered` is
/// [`::calendar::eventkit::access_answered`] (always `true` off macOS, where
/// there is no prompt).
pub fn needs_eventkit_answer(configured: &[Provider], eventkit_answered: bool) -> bool {
    configured.contains(&Provider::EventKit) && !eventkit_answered
}

/// The configured providers this read may ask: all of them, minus
/// Calendar.app while its prompt is unanswered and the read must not prompt.
pub fn readable_providers(
    configured: &[Provider],
    eventkit_answered: bool,
    prompt: Prompt,
) -> Vec<Provider> {
    let skip_eventkit =
        prompt == Prompt::Never && needs_eventkit_answer(configured, eventkit_answered);
    let mut providers: Vec<Provider> = configured
        .iter()
        .copied()
        .filter(|provider| !(skip_eventkit && *provider == Provider::EventKit))
        .collect();
    // TUR-49: EventKit first, so a meeting it shares with a sign-in keeps
    // EventKit's id (`merge_events` keeps the first source's).
    providers.sort_by_key(|provider| *provider != Provider::EventKit);
    providers
}

/// [`readable_providers`], built: `build` makes each one, and leaves out one
/// that cannot be read at all (a cloud calendar with no sign-in).
pub fn pick(
    configured: &[Provider],
    eventkit_answered: bool,
    prompt: Prompt,
    build: impl Fn(Provider) -> Option<SharedProvider>,
) -> Vec<SharedProvider> {
    readable_providers(configured, eventkit_answered, prompt)
        .into_iter()
        .filter_map(build)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Prompt; 2] = [Prompt::Allowed, Prompt::Never];

    #[test]
    fn cloud_only_reads_every_sign_in_whatever_eventkit_says() {
        for configured in [
            vec![Provider::Google],
            vec![Provider::Microsoft],
            vec![Provider::Microsoft, Provider::Google],
        ] {
            assert!(!needs_eventkit_answer(&configured, false));
            for prompt in ALL {
                assert_eq!(readable_providers(&configured, false, prompt), configured);
            }
        }
    }

    #[test]
    fn mixed_with_eventkit_unanswered_reads_the_sign_in_in_the_background() {
        let configured = [Provider::Google, Provider::EventKit];
        assert!(needs_eventkit_answer(&configured, false));
        assert_eq!(
            readable_providers(&configured, false, Prompt::Never),
            [Provider::Google]
        );
        // The Today pane asks, EventKit first.
        assert_eq!(
            readable_providers(&configured, false, Prompt::Allowed),
            [Provider::EventKit, Provider::Google]
        );
    }

    #[test]
    fn eventkit_answered_is_read_everywhere() {
        let configured = [Provider::Microsoft, Provider::EventKit];
        assert!(!needs_eventkit_answer(&configured, true));
        for prompt in ALL {
            assert_eq!(
                readable_providers(&configured, true, prompt),
                [Provider::EventKit, Provider::Microsoft]
            );
        }
    }

    #[test]
    fn eventkit_alone_and_unanswered_leaves_nothing_in_the_background() {
        let configured = [Provider::EventKit];
        assert!(readable_providers(&configured, false, Prompt::Never).is_empty());
        assert_eq!(
            readable_providers(&configured, false, Prompt::Allowed),
            [Provider::EventKit]
        );
    }

    #[test]
    fn pick_leaves_out_what_cannot_be_built() {
        use ::calendar::fake::FakeProvider;
        let built = pick(
            &[Provider::Google, Provider::Microsoft],
            false,
            Prompt::Never,
            |provider| {
                (provider == Provider::Microsoft)
                    .then(|| std::sync::Arc::new(FakeProvider::with_events([])) as SharedProvider)
            },
        );
        assert_eq!(built.len(), 1);
    }
}
