//! Agent CLI runs in flight, each under a key with its cancel handle, so a
//! second press on the same thing is refused and quitting can stop them all
//! (TUR-160). Used by Sync (`sync::SyncRuns`) and the agent Test run
//! (`agent_setup::TestRuns`); the notes run has its own (`agent_run::runs`),
//! which [`Cancels::shutdown`] follows.
//!
//! Cancelling a run is what kills its CLI: the run's own thread sees the flag
//! within `agent::process`'s poll and stops the whole process tree
//! (`ProcessTree::stop`: `kill_tree`, then a reap). A claim is dropped only
//! once its run has returned, so waiting for every claim to go is waiting for
//! every tree to be gone. Nothing else stops them: on unix the CLI leads its
//! own process group, so it outlives an app that exits without this.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use agent::CancelHandle;

use crate::lock::lock_or_recover;

/// Why [`Cancels::claim`] said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// A run under this key is going.
    Busy,
    /// The app is quitting ([`Cancels::shutdown`]).
    Closed,
}

/// The runs going, by key.
#[derive(Debug)]
pub struct Cancels<K> {
    inner: Mutex<Inner<K>>,
    /// Signalled whenever a claim is dropped, for [`Cancels::shutdown`].
    freed: Condvar,
}

#[derive(Debug)]
struct Inner<K> {
    going: HashMap<K, CancelHandle>,
    /// The app is quitting: claim nothing more.
    closed: bool,
}

impl<K> Default for Cancels<K> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                going: HashMap::new(),
                closed: false,
            }),
            freed: Condvar::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> Cancels<K> {
    fn lock(&self) -> MutexGuard<'_, Inner<K>> {
        // Only cancel flags in here, so a panic mid-insert leaves nothing
        // half-written worth refusing over.
        lock_or_recover(&self.inner)
    }

    /// Marks `key` as running until the returned claim is dropped.
    pub fn claim(&self, key: K) -> Result<Claim<'_, K>, Refused> {
        let mut inner = self.lock();
        if inner.closed {
            return Err(Refused::Closed);
        }
        if inner.going.contains_key(&key) {
            return Err(Refused::Busy);
        }
        let cancel = CancelHandle::new();
        inner.going.insert(key.clone(), cancel.clone());
        Ok(Claim {
            cancels: self,
            key,
            cancel,
        })
    }

    /// Stops the run under `key`, if there is one.
    pub fn cancel(&self, key: &K) {
        if let Some(cancel) = self.lock().going.get(key) {
            cancel.cancel();
        }
    }

    /// The keys with a run going.
    pub fn keys(&self) -> Vec<K> {
        self.lock().going.keys().cloned().collect()
    }

    /// The app is quitting: claim nothing more, cancel every run going, and
    /// wait up to `wait` for all of them to end. Returns how many are still
    /// going after that, for the caller's log.
    pub fn shutdown(&self, wait: Duration) -> usize {
        let mut inner = self.lock();
        inner.closed = true;
        inner.going.values().for_each(CancelHandle::cancel);
        let (inner, _) = self
            .freed
            .wait_timeout_while(inner, wait, |inner| !inner.going.is_empty())
            .unwrap_or_else(PoisonError::into_inner);
        inner.going.len()
    }
}

/// One run's place in [`Cancels`]; dropping it frees the key.
#[derive(Debug)]
pub struct Claim<'a, K: Eq + Hash> {
    cancels: &'a Cancels<K>,
    key: K,
    /// Hand this to the run's `Job`.
    pub cancel: CancelHandle,
}

impl<K: Eq + Hash> Drop for Claim<'_, K> {
    fn drop(&mut self) {
        lock_or_recover(&self.cancels.inner).going.remove(&self.key);
        self.cancels.freed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn a_key_runs_once_at_a_time_and_is_free_again_after() {
        let cancels = Cancels::default();
        let first = cancels.claim("a").unwrap();
        assert_eq!(cancels.claim("a").err(), Some(Refused::Busy));
        let other = cancels.claim("b").unwrap();
        cancels.cancel(&"a");
        assert!(first.cancel.is_cancelled());
        assert!(!other.cancel.is_cancelled());
        drop(first);
        assert!(cancels.claim("a").is_ok());
        assert_eq!(cancels.keys(), vec!["b"]);
    }

    #[test]
    fn shutdown_cancels_every_run_and_waits_for_them_to_end() {
        let cancels = Cancels::default();
        std::thread::scope(|scope| {
            for key in ["a", "b"] {
                let claim = cancels.claim(key).unwrap();
                // A run that only ends when it is cancelled, like a CLI
                // that never answers.
                scope.spawn(move || {
                    while !claim.cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    std::thread::sleep(Duration::from_millis(30));
                    drop(claim);
                });
            }
            let started = Instant::now();
            assert_eq!(cancels.shutdown(Duration::from_secs(10)), 0);
            assert!(started.elapsed() < Duration::from_secs(5));
            assert!(cancels.keys().is_empty());
        });
        assert_eq!(cancels.claim("c").err(), Some(Refused::Closed));
    }

    #[test]
    fn shutdown_gives_up_on_a_run_that_does_not_end_in_time() {
        let cancels = Cancels::default();
        let stuck = cancels.claim(1).unwrap();
        let started = Instant::now();
        assert_eq!(cancels.shutdown(Duration::from_millis(50)), 1);
        assert!(started.elapsed() >= Duration::from_millis(50));
        assert!(stuck.cancel.is_cancelled());
    }
}
