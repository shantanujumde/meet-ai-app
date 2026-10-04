//! Which render endpoint the Windows loopback follows, as pure functions
//! (TUR-95).
//!
//! A call app can play through an output that is not the system default (a
//! headset kept as the "communications" device, or picked in the app's own
//! settings). Loopback on the default then records nothing of the other side.
//! So every read lists which render endpoints other apps are playing to and
//! picks one with [`endpoint_to_follow`]; the id it returns is what the
//! existing switch policy (`crate::loopback::follower::DeviceWatch`: a switch
//! only after two agreeing reads) sees, exactly like a default-device change.
//!
//! Compiled on every OS so the rules are unit-tested on the Mac that builds
//! this crate; only `windows/render_in_use.rs` reads the real WASAPI sessions
//! and `windows_loopback.rs` opens the endpoint chosen here.

use crate::activity::{AppStream, other_apps};

/// One active render endpoint and the audio sessions open on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderEndpoint {
    /// The WASAPI endpoint id (`IMMDevice::GetId`), the same string `cpal`'s
    /// `DeviceId` carries.
    pub id: String,
    /// Every session on the endpoint, active or not, with its pid.
    pub sessions: Vec<AppStream>,
}

impl RenderEndpoint {
    /// Another app is playing here: an `Active` session that is neither
    /// meet-ai's own (`own_pid`: its silence keepalive and start sound) nor
    /// the system-sounds session (pid 0).
    fn playing(&self, own_pid: u32) -> bool {
        other_apps(&self.sessions, own_pid).next().is_some()
    }
}

// Adapted from github.com/fastrepl/anarlog/crates/audio-actual/src/speaker/windows.rs @ 93deb8642e75a0a2f8ece1bed186da4362213edd (MIT)
/// The endpoint to record, given the default render endpoint's id and every
/// active render endpoint:
///
/// * another app plays to the default: the default (it wins ties, so a video
///   on a second screen's speakers does not pull the recording away from a
///   call on the default);
/// * another app plays to exactly one other endpoint: that one;
/// * several other endpoints play, or none: the default.
///
/// `None` only when there is no default endpoint and no single endpoint is
/// playing; the watch then keeps what it followed so far.
pub(crate) fn endpoint_to_follow(
    default: Option<&str>,
    endpoints: &[RenderEndpoint],
    own_pid: u32,
) -> Option<String> {
    let default_playing = default.is_some_and(|id| {
        endpoints
            .iter()
            .any(|endpoint| endpoint.id == id && endpoint.playing(own_pid))
    });
    if default_playing {
        return default.map(str::to_string);
    }
    let mut others = endpoints
        .iter()
        .filter(|endpoint| Some(endpoint.id.as_str()) != default)
        .filter(|endpoint| endpoint.playing(own_pid));
    match (others.next(), others.next()) {
        (Some(only), None) => Some(only.id.clone()),
        _ => default.map(str::to_string),
    }
}

/// The endpoint id the loopback has to open by name, or `None` to open the
/// default device the usual way. The default is opened through `cpal`'s
/// default device, which Windows reroutes by itself when the default
/// changes; only a non-default endpoint needs opening by id.
pub(crate) fn endpoint_to_open<'a>(
    followed: Option<&'a str>,
    default: Option<&str>,
) -> Option<&'a str> {
    followed.filter(|id| Some(*id) != default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::follower::DeviceWatch;

    const OWN: u32 = 4242;
    const SPEAKERS: &str = "{0.0.0.00000000}.{speakers}";
    const HEADSET: &str = "{0.0.0.00000000}.{headset}";
    const MONITOR: &str = "{0.0.0.00000000}.{monitor}";

    fn session(pid: u32, active: bool) -> AppStream {
        AppStream {
            pid: Some(pid),
            name: String::new(),
            active,
        }
    }

    fn endpoint(id: &str, sessions: Vec<AppStream>) -> RenderEndpoint {
        RenderEndpoint {
            id: id.to_string(),
            sessions,
        }
    }

    /// Speakers (the default), a headset and a monitor; `playing` names the
    /// ones another app (pid 100, 200, ...) has an active session on.
    fn devices(playing: &[&str]) -> Vec<RenderEndpoint> {
        [SPEAKERS, HEADSET, MONITOR]
            .iter()
            .zip(1u32..)
            .map(|(id, n)| {
                let mut sessions = vec![session(n * 100 + 1, false)];
                if playing.contains(id) {
                    sessions.push(session(n * 100, true));
                }
                endpoint(id, sessions)
            })
            .collect()
    }

    fn follow(playing: &[&str]) -> Option<String> {
        endpoint_to_follow(Some(SPEAKERS), &devices(playing), OWN)
    }

    #[test]
    fn nothing_playing_follows_the_default() {
        assert_eq!(follow(&[]).as_deref(), Some(SPEAKERS));
    }

    #[test]
    fn one_non_default_device_playing_is_followed() {
        assert_eq!(follow(&[HEADSET]).as_deref(), Some(HEADSET));
    }

    #[test]
    fn the_default_playing_wins_over_another_device() {
        assert_eq!(follow(&[SPEAKERS]).as_deref(), Some(SPEAKERS));
        assert_eq!(follow(&[SPEAKERS, HEADSET]).as_deref(), Some(SPEAKERS));
    }

    #[test]
    fn several_non_default_devices_playing_follow_the_default() {
        assert_eq!(follow(&[HEADSET, MONITOR]).as_deref(), Some(SPEAKERS));
    }

    #[test]
    fn our_own_and_system_sound_sessions_do_not_count() {
        // meet-ai's keepalive on the headset, the system-sounds session
        // (pid 0) on the monitor: neither is another app playing.
        let endpoints = vec![
            endpoint(SPEAKERS, vec![]),
            endpoint(HEADSET, vec![session(OWN, true)]),
            endpoint(MONITOR, vec![session(0, true)]),
        ];
        assert_eq!(
            endpoint_to_follow(Some(SPEAKERS), &endpoints, OWN).as_deref(),
            Some(SPEAKERS)
        );
        // Our own session on the default does not make it "playing" either.
        let endpoints = vec![
            endpoint(SPEAKERS, vec![session(OWN, true)]),
            endpoint(HEADSET, vec![session(100, true)]),
        ];
        assert_eq!(
            endpoint_to_follow(Some(SPEAKERS), &endpoints, OWN).as_deref(),
            Some(HEADSET)
        );
    }

    #[test]
    fn an_inactive_session_is_not_playing() {
        let endpoints = vec![
            endpoint(SPEAKERS, vec![]),
            endpoint(HEADSET, vec![session(100, false)]),
        ];
        assert_eq!(
            endpoint_to_follow(Some(SPEAKERS), &endpoints, OWN).as_deref(),
            Some(SPEAKERS)
        );
    }

    #[test]
    fn a_default_missing_from_the_list_still_counts_as_the_default() {
        // The collection read raced a device change: the default is not in it.
        let endpoints = vec![endpoint(HEADSET, vec![session(100, true)])];
        assert_eq!(
            endpoint_to_follow(Some(SPEAKERS), &endpoints, OWN).as_deref(),
            Some(HEADSET)
        );
        assert_eq!(
            endpoint_to_follow(Some(SPEAKERS), &[], OWN).as_deref(),
            Some(SPEAKERS)
        );
    }

    #[test]
    fn without_a_default_only_a_single_playing_device_is_an_answer() {
        assert_eq!(
            endpoint_to_follow(None, &devices(&[HEADSET]), OWN).as_deref(),
            Some(HEADSET)
        );
        assert_eq!(endpoint_to_follow(None, &devices(&[]), OWN), None);
        assert_eq!(
            endpoint_to_follow(None, &devices(&[HEADSET, MONITOR]), OWN),
            None
        );
    }

    #[test]
    fn only_a_non_default_endpoint_is_opened_by_id() {
        assert_eq!(
            endpoint_to_open(Some(HEADSET), Some(SPEAKERS)),
            Some(HEADSET)
        );
        assert_eq!(endpoint_to_open(Some(SPEAKERS), Some(SPEAKERS)), None);
        assert_eq!(endpoint_to_open(None, Some(SPEAKERS)), None);
        assert_eq!(endpoint_to_open(Some(HEADSET), None), Some(HEADSET));
    }

    /// The choice through the unchanged switch policy, the way
    /// `windows_devices::default_output_device` runs it: polled every 200 ms,
    /// read every 2 s, a switch once two reads agree.
    fn watched(playing_at: impl Fn(u64) -> Vec<&'static str>) -> Vec<(u64, Option<String>)> {
        let mut watch = DeviceWatch::new();
        (0..20_000u64)
            .step_by(200)
            .map(|ms| {
                let answer = watch.poll(ms * 1_000_000, || {
                    endpoint_to_follow(Some(SPEAKERS), &devices(&playing_at(ms)), OWN)
                });
                (ms, answer)
            })
            .collect()
    }

    fn first_tick_on(seen: &[(u64, Option<String>)], id: &str) -> Option<u64> {
        seen.iter()
            .find(|(_, answer)| answer.as_deref() == Some(id))
            .map(|(ms, _)| *ms)
    }

    #[test]
    fn a_call_on_the_headset_is_followed_after_two_agreeing_reads() {
        // The call starts playing to the headset at 3.1 s: read at 4 s (1st),
        // 6 s (2nd, confirmed). It ends at 11 s: back at 14 s.
        let seen = watched(|ms| {
            if (3_100..11_000).contains(&ms) {
                vec![HEADSET]
            } else {
                vec![]
            }
        });
        assert_eq!(seen[0].1.as_deref(), Some(SPEAKERS));
        assert_eq!(first_tick_on(&seen, HEADSET), Some(6_000));
        let back = seen
            .iter()
            .skip_while(|(ms, _)| *ms < 6_000)
            .find(|(_, answer)| answer.as_deref() == Some(SPEAKERS))
            .map(|(ms, _)| *ms);
        assert_eq!(back, Some(14_000));
        let flips = seen.windows(2).filter(|w| w[0].1 != w[1].1).count();
        assert_eq!(flips, 2);
    }

    #[test]
    fn a_short_sound_on_another_device_is_not_followed() {
        // A notification on the headset seen by one read only (4 s).
        let seen = watched(|ms| {
            if (3_100..5_000).contains(&ms) {
                vec![HEADSET]
            } else {
                vec![]
            }
        });
        assert_eq!(first_tick_on(&seen, HEADSET), None);
    }

    #[test]
    fn the_default_starting_to_play_takes_the_recording_back() {
        // The call on the headset, then a second app on the default from 9 s.
        let seen = watched(|ms| {
            let mut playing = vec![];
            if ms >= 3_100 {
                playing.push(HEADSET);
            }
            if ms >= 9_000 {
                playing.push(SPEAKERS);
            }
            playing
        });
        assert_eq!(first_tick_on(&seen, HEADSET), Some(6_000));
        assert_eq!(seen.last().and_then(|(_, a)| a.as_deref()), Some(SPEAKERS));
    }
}
