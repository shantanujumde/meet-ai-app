//! The recording path's memory of a usable `meet-stt --probe` (TUR-172).
//!
//! Every recording used to run the sidecar's probe again before building its
//! engine, which ate into the time the live tee buffers while the engine
//! opens. [`super::select`] now keeps a probe that said Apple's engine is
//! usable offline, per sidecar binary and locale, for the rest of the
//! process: that answer does not go stale on its own. Any other answer (an
//! error, the OS too old, the locale's model not installed yet) is not kept,
//! so a model that finishes installing is used from the next recording on.
//!
//! [`super::resolve`] and [`super::options`] (Settings) never read it: they
//! show `installed_locales`, which installing another locale changes.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::Error;
use crate::apple::{AppleEngine, Probe};

/// Usable probes by `(sidecar binary, locale)`. A list, not a map: there is
/// one entry in practice, and `Vec::new` lets it be a `static`.
pub(super) struct ProbeCache {
    usable: Mutex<Vec<((PathBuf, String), Probe)>>,
}

impl ProbeCache {
    pub(super) const fn new() -> Self {
        Self {
            usable: Mutex::new(Vec::new()),
        }
    }

    /// The kept probe for `binary` and `locale`, or `run`'s answer, kept
    /// when it is usable offline. `run` goes without the lock held, so a slow
    /// sidecar never holds up another caller.
    pub(super) fn probe(
        &self,
        binary: &Path,
        locale: &str,
        run: impl FnOnce() -> Result<Probe, Error>,
    ) -> Result<Probe, Error> {
        let key = (binary.to_path_buf(), locale.to_string());
        if let Some(probe) = self.lookup(&key) {
            return Ok(probe);
        }
        let probe = run();
        if let Ok(answer) = &probe
            && answer.is_usable_offline()
        {
            let mut usable = self.usable.lock().unwrap_or_else(PoisonError::into_inner);
            if !usable.iter().any(|(kept, _)| *kept == key) {
                usable.push((key, answer.clone()));
            }
        }
        probe
    }

    fn lookup(&self, key: &(PathBuf, String)) -> Option<Probe> {
        self.usable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(kept, _)| kept == key)
            .map(|(_, probe)| probe.clone())
    }
}

static CACHE: ProbeCache = ProbeCache::new();

/// [`AppleEngine::probe`] through the process-wide [`ProbeCache`].
pub(super) fn probe(binary: &Path, locale: &str) -> Result<Probe, Error> {
    CACHE.probe(binary, locale, || AppleEngine::probe(binary, locale))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn answer(available: bool, installed: bool) -> Probe {
        Probe {
            available,
            installed,
            locale: Some("en-US".into()),
            reason: None,
            os_version: None,
            installed_locales: Vec::new(),
        }
    }

    #[test]
    fn a_usable_probe_runs_the_sidecar_once() {
        let cache = ProbeCache::new();
        let runs = Cell::new(0);
        let run = || {
            runs.set(runs.get() + 1);
            Ok(answer(true, true))
        };
        let binary = Path::new("/bundle/meet-stt");
        assert!(
            cache
                .probe(binary, "en-US", run)
                .unwrap()
                .is_usable_offline()
        );
        assert!(
            cache
                .probe(binary, "en-US", run)
                .unwrap()
                .is_usable_offline()
        );
        assert_eq!(runs.get(), 1);

        // Another locale or binary is its own question.
        cache.probe(binary, "de-DE", run).unwrap();
        cache
            .probe(Path::new("/other/meet-stt"), "en-US", run)
            .unwrap();
        assert_eq!(runs.get(), 3);
    }

    #[test]
    fn an_unusable_answer_or_an_error_is_asked_again() {
        let cache = ProbeCache::new();
        let runs = Cell::new(0);
        let binary = Path::new("/bundle/meet-stt");
        let missing = || {
            runs.set(runs.get() + 1);
            Ok(answer(true, false))
        };
        cache.probe(binary, "en-US", missing).unwrap();
        cache.probe(binary, "en-US", missing).unwrap();
        let failing = || {
            runs.set(runs.get() + 1);
            Err(Error::Sidecar("exit status 1".into()))
        };
        assert!(cache.probe(binary, "en-US", failing).is_err());
        assert_eq!(runs.get(), 3);

        // Once the model is installed, that answer is kept.
        let installed = || {
            runs.set(runs.get() + 1);
            Ok(answer(true, true))
        };
        cache.probe(binary, "en-US", installed).unwrap();
        cache.probe(binary, "en-US", installed).unwrap();
        assert_eq!(runs.get(), 4);
    }
}
