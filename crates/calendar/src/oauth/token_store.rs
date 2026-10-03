//! Where the refresh token lives between launches (SPEC §2.7): the OS
//! keystore and nowhere else. Never `config.jsonc`, never a file.
//!
//! Only the refresh token string is stored, never the whole token response:
//! Windows Credential Manager caps a secret at 2560 bytes, and the access
//! token stays in memory anyway.

use super::ProviderId;

/// The keystore service name: the bundle id, frozen (SETUP.md step 1).
pub const KEYRING_SERVICE: &str = "pro.saleschat.meetai";

/// The keystore user for a provider: `calendar-google`, `calendar-microsoft`.
pub fn keyring_user(provider: ProviderId) -> String {
    format!("calendar-{}", provider.as_str())
}

/// The keystore could not be read or written. On Linux this is usually "no
/// Secret Service running" (no gnome-keyring or KWallet).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the OS keystore is not available: {0}")]
pub struct StoreError(pub String);

/// Load, save and delete one refresh token per provider.
pub trait TokenStore: Send + Sync {
    /// The stored refresh token, or `Ok(None)` when there is none.
    fn load(&self, provider: ProviderId) -> Result<Option<String>, StoreError>;
    /// Replace the stored refresh token.
    fn save(&self, provider: ProviderId, refresh_token: &str) -> Result<(), StoreError>;
    /// Remove it. Deleting a token that is not there is not an error.
    fn delete(&self, provider: ProviderId) -> Result<(), StoreError>;
}

/// The real store: macOS Keychain, Windows Credential Manager, or the Secret
/// Service on Linux, picked by `keyring` 4's `v1` API.
///
/// Never used by tests (they must not touch the real keychain); see
/// [`MemoryStore`].
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(provider: ProviderId) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(KEYRING_SERVICE, &keyring_user(provider))
            .map_err(|error| StoreError(error.to_string()))
    }
}

impl TokenStore for KeyringStore {
    fn load(&self, provider: ProviderId) -> Result<Option<String>, StoreError> {
        match Self::entry(provider)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(StoreError(error.to_string())),
        }
    }

    fn save(&self, provider: ProviderId, refresh_token: &str) -> Result<(), StoreError> {
        Self::entry(provider)?
            .set_password(refresh_token)
            .map_err(|error| StoreError(error.to_string()))
    }

    fn delete(&self, provider: ProviderId) -> Result<(), StoreError> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(StoreError(error.to_string())),
        }
    }
}

/// An in-memory store for tests, so no test ever touches the real keychain.
///
/// [`MemoryStore::unavailable`] behaves like Linux with no Secret Service:
/// every call fails.
#[cfg(any(test, feature = "fake"))]
#[derive(Debug, Default)]
pub struct MemoryStore {
    tokens: std::sync::Mutex<std::collections::HashMap<ProviderId, String>>,
    unavailable: bool,
}

#[cfg(any(test, feature = "fake"))]
impl MemoryStore {
    /// A store where every call fails, like a Linux desktop with no Secret
    /// Service running.
    pub fn unavailable() -> Self {
        Self {
            unavailable: true,
            ..Self::default()
        }
    }

    /// What is stored for `provider`, read directly, for assertions.
    pub fn stored(&self, provider: ProviderId) -> Option<String> {
        self.lock().get(&provider).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<ProviderId, String>> {
        self.tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn check(&self) -> Result<(), StoreError> {
        if self.unavailable {
            return Err(StoreError("no Secret Service running (test)".into()));
        }
        Ok(())
    }
}

#[cfg(any(test, feature = "fake"))]
impl TokenStore for MemoryStore {
    fn load(&self, provider: ProviderId) -> Result<Option<String>, StoreError> {
        self.check()?;
        Ok(self.stored(provider))
    }

    fn save(&self, provider: ProviderId, refresh_token: &str) -> Result<(), StoreError> {
        self.check()?;
        self.lock().insert(provider, refresh_token.to_owned());
        Ok(())
    }

    fn delete(&self, provider: ProviderId) -> Result<(), StoreError> {
        self.check()?;
        self.lock().remove(&provider);
        Ok(())
    }
}
