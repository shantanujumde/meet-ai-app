//! One poison-recovering lock, shared by every module that guards state with
//! a `Mutex`.

use std::sync::{Mutex, MutexGuard};

/// Lock `mutex`, recovering the guard if a previous holder panicked.
///
/// A poisoned lock here means an earlier call panicked while holding it.
/// Recovering is strictly better than taking the whole app down; the worst
/// case is a stuck phase, not a half-broken invariant.
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
