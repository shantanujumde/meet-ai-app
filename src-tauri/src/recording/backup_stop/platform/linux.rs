//! Linux: logind's `PrepareForSleep` (TUR-145).
//!
//! systemd-logind sends `org.freedesktop.login1.Manager.PrepareForSleep`
//! with `true` before a suspend or hibernate and `false` after the wake. A
//! signal alone gives no time: the machine may be asleep before the stop
//! has run. So meet-ai holds a "delay" inhibitor lock (`Inhibit("sleep",
//! ..., "delay")`): logind waits until it is released, or until its
//! `InhibitDelayMaxSec` (5 s by default, set by the distribution, so it is
//! not relied on beyond [`BUDGET`]). The lock is released once the stop has
//! run or [`BUDGET`] is up, and taken again after the wake.
//!
//! A system without logind (no system bus, another init) gets no sleep stop;
//! that is logged once and the app runs on.

use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedFd;

use super::{Handler, run_within};

/// How long the stop may hold the sleep up, under logind's default 5 s.
const BUDGET: Duration = Duration::from_secs(4);

const LOGIND: &str = "org.freedesktop.login1";
const LOGIND_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

pub(super) fn on_will_sleep(handler: Handler) -> Result<(), String> {
    let connection =
        Connection::system().map_err(|error| format!("no system bus for logind: {error}"))?;
    let proxy = Proxy::new(&connection, LOGIND, LOGIND_PATH, MANAGER)
        .map_err(|error| format!("could not reach logind: {error}"))?;
    let signals = proxy
        .receive_signal("PrepareForSleep")
        .map_err(|error| format!("could not listen for PrepareForSleep: {error}"))?;
    let first_lock = inhibit(&proxy);
    std::thread::Builder::new()
        .name("meet-ai-sleep-watch".to_owned())
        .spawn(move || {
            // The proxy is kept on this thread for the inhibitor calls; the
            // connection lives inside it.
            let mut lock = first_lock;
            for message in signals {
                let Ok(going_to_sleep) = message.body().deserialize::<bool>() else {
                    continue;
                };
                if going_to_sleep {
                    tracing::info!("the computer is going to sleep");
                    if !run_within(&handler, BUDGET) {
                        tracing::warn!(
                            "the sleep stop is still running; it finishes after the wake"
                        );
                    }
                    // Dropping the descriptor releases the lock: sleep now.
                    lock = None;
                } else if lock.is_none() {
                    lock = inhibit(&proxy);
                }
            }
            tracing::warn!("logind's signals ended; no sleep stop from now on");
        })
        .map(drop)
        .map_err(|error| format!("could not start the sleep watch: {error}"))
}

/// Take a delay inhibitor for sleep. `None` (logged) when logind refuses:
/// the signal still arrives, only without the guaranteed few seconds.
fn inhibit(proxy: &Proxy<'_>) -> Option<OwnedFd> {
    proxy
        .call::<_, _, OwnedFd>(
            "Inhibit",
            &(
                "sleep",
                "meet-ai",
                "Saving the recording before sleep",
                "delay",
            ),
        )
        .inspect_err(|error| tracing::warn!(%error, "logind would not delay sleep for meet-ai"))
        .ok()
}
