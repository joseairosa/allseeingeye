//! OS keychain integration for the device backup keypair.
//!
//! Uses the `keyring` crate to talk to:
//! * macOS Keychain on Darwin,
//! * Windows Credential Manager on Windows,
//! * libsecret / kwallet on Linux.
//!
//! The stored value is the 32-byte X25519 private key encoded as 64
//! lowercase hex characters - the `keyring` API stores strings, not
//! raw bytes, so hex is the lowest-friction encoding. Loaders
//! reconstruct the private key from hex on every unwrap; the bytes
//! never live in process memory beyond the call stack of the caller
//! (spec 15.2 - "we never cache it in process memory across
//! operations").

use crate::backup::envelope::{hex_decode, hex_encode};

/// Keychain service identifier - the namespace under which the entry
/// lives.
pub const BACKUP_KEYCHAIN_SERVICE: &str = "dev.allseeingeye.backup";

/// Keychain account identifier - the entry name within the service.
pub const BACKUP_KEYCHAIN_ACCOUNT: &str = "device-key";

/// Errors raised by the keychain layer.
///
/// `Unavailable` covers the "Linux without libsecret" case so the IPC
/// layer can degrade gracefully (disable the backup buttons + show a
/// recovery hint) instead of crashing.
#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("keychain backend is unavailable on this platform: {0}")]
    Unavailable(String),

    #[error("keychain entry not found")]
    NotFound,

    #[error("keychain entry exists but contains malformed key material")]
    MalformedEntry,

    #[error("keychain access failed: {0}")]
    Access(String),
}

impl From<keyring::Error> for KeychainError {
    fn from(err: keyring::Error) -> Self {
        match err {
            keyring::Error::NoEntry => KeychainError::NotFound,
            keyring::Error::PlatformFailure(inner) => KeychainError::Unavailable(inner.to_string()),
            keyring::Error::NoStorageAccess(inner) => KeychainError::Access(inner.to_string()),
            other => KeychainError::Access(other.to_string()),
        }
    }
}

/// Trait that abstracts the platform keychain so tests can use an
/// in-memory fake. Production code consumes [`SystemKeychain`].
pub trait Keychain {
    fn get_private_key_hex(&self) -> Result<String, KeychainError>;
    fn set_private_key_hex(&self, hex: &str) -> Result<(), KeychainError>;
}

/// Helper: decode a hex private key from the keychain into the raw
/// 32-byte form. Returns `MalformedEntry` if the hex is invalid.
pub fn decode_private_key(hex: &str) -> Result<[u8; 32], KeychainError> {
    let bytes = hex_decode(hex).ok_or(KeychainError::MalformedEntry)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| KeychainError::MalformedEntry)?;
    Ok(arr)
}

/// Helper: hex-encode a freshly-generated 32-byte private key for
/// keychain storage.
#[must_use]
pub fn encode_private_key(bytes: &[u8; 32]) -> String {
    hex_encode(bytes)
}

/// Production keychain backed by the OS via the `keyring` crate.
pub struct SystemKeychain;

impl SystemKeychain {
    fn entry() -> Result<keyring::Entry, KeychainError> {
        keyring::Entry::new(BACKUP_KEYCHAIN_SERVICE, BACKUP_KEYCHAIN_ACCOUNT)
            .map_err(KeychainError::from)
    }
}

impl Keychain for SystemKeychain {
    fn get_private_key_hex(&self) -> Result<String, KeychainError> {
        let entry = Self::entry()?;
        let value = entry.get_password().map_err(KeychainError::from)?;
        Ok(value)
    }

    fn set_private_key_hex(&self, hex: &str) -> Result<(), KeychainError> {
        let entry = Self::entry()?;
        entry.set_password(hex).map_err(KeychainError::from)?;
        Ok(())
    }
}

/// In-memory test keychain shared across the backup unit suite AND
/// the integration test under `tests/`.
///
/// Lives at module scope (rather than inside `#[cfg(test)] mod tests`)
/// because integration tests under `tests/` compile against the crate
/// as a downstream consumer and never see anything `#[cfg(test)]`-
/// gated. The helper is documented as `__test_only_*` at the crate
/// root so consumers cannot accidentally rely on it from production
/// code.
#[doc(hidden)]
pub mod test_support {
    use super::{Keychain, KeychainError};
    use std::sync::Mutex;

    /// Simple atomic store backed by a `Mutex<Option<String>>` so the
    /// keychain trait surface is exercised end-to-end without touching
    /// the OS keychain.
    pub struct InMemoryKeychain {
        inner: Mutex<Option<String>>,
    }

    impl Default for InMemoryKeychain {
        fn default() -> Self {
            Self::new()
        }
    }

    impl InMemoryKeychain {
        #[must_use]
        pub fn new() -> Self {
            Self {
                inner: Mutex::new(None),
            }
        }
    }

    impl Keychain for InMemoryKeychain {
        fn get_private_key_hex(&self) -> Result<String, KeychainError> {
            self.inner
                .lock()
                .unwrap()
                .clone()
                .ok_or(KeychainError::NotFound)
        }

        fn set_private_key_hex(&self, hex: &str) -> Result<(), KeychainError> {
            *self.inner.lock().unwrap() = Some(hex.to_owned());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::keychain::test_support::InMemoryKeychain;

    #[test]
    fn in_memory_keychain_round_trip() {
        let kc = InMemoryKeychain::new();
        assert!(matches!(
            kc.get_private_key_hex(),
            Err(KeychainError::NotFound)
        ));
        kc.set_private_key_hex("abc123").expect("set");
        assert_eq!(kc.get_private_key_hex().unwrap(), "abc123");
    }

    #[test]
    fn decode_rejects_short_hex() {
        let err = decode_private_key("deadbeef").expect_err("short hex");
        assert!(matches!(err, KeychainError::MalformedEntry));
    }

    #[test]
    fn decode_rejects_garbage() {
        let err = decode_private_key("not hex characters").expect_err("garbage");
        assert!(matches!(err, KeychainError::MalformedEntry));
    }

    #[test]
    fn encode_then_decode_round_trip() {
        let original = [0x42u8; 32];
        let hex = encode_private_key(&original);
        assert_eq!(hex.len(), 64);
        let decoded = decode_private_key(&hex).expect("decode");
        assert_eq!(decoded, original);
    }
}
