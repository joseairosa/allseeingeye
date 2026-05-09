//! `ensure_keypair` orchestrator.
//!
//! Bridges the keychain layer ([`crate::backup::keychain`]) and the
//! cached public key in `app_settings.backupPublicKey`. The contract:
//!
//! 1. If a keychain entry exists, decode the private key, recompute
//!    the public, and refresh the `app_settings` cache so the two
//!    stay in sync. Return the public.
//! 2. If no keychain entry exists, generate a fresh X25519 keypair,
//!    persist private to keychain, public to `app_settings`. Return
//!    the public.
//!
//! Idempotent: calling it twice in a row returns the same public key
//! and never overwrites the keychain entry.

use rand_core::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::backup::envelope::DevicePublicKey;
use crate::backup::keychain::{
    decode_private_key, encode_private_key, Keychain, KeychainError, SystemKeychain,
};
use crate::index::settings::{read_setting_raw, write_setting_raw, KEY_BACKUP_PUBLIC_KEY};
use crate::index::IndexHandle;

/// Errors raised by [`ensure_keypair`].
#[derive(Debug, thiserror::Error)]
pub enum KeypairError {
    #[error(transparent)]
    Keychain(#[from] KeychainError),

    #[error(transparent)]
    Index(#[from] crate::index::IndexError),
}

/// Ensure a device backup keypair exists.
///
/// Returns the device public key (safe to cache, not secret). The
/// private key never leaves this function: it is hex-decoded from the
/// keychain into a stack-local `StaticSecret`, used only to recompute
/// the matching public, and dropped at function exit.
pub fn ensure_keypair(handle: &IndexHandle) -> Result<DevicePublicKey, KeypairError> {
    let keychain = SystemKeychain;
    ensure_keypair_with(handle, &keychain)
}

/// Test seam - identical to [`ensure_keypair`] but takes a custom
/// keychain so unit tests can exercise the orchestration without a
/// real OS keychain.
pub fn ensure_keypair_with<K: Keychain>(
    handle: &IndexHandle,
    keychain: &K,
) -> Result<DevicePublicKey, KeypairError> {
    match keychain.get_private_key_hex() {
        Ok(hex) => {
            // Existing entry. Recompute the public side and refresh
            // the app_settings cache (which may have been wiped by a
            // /spec reset_index, etc.).
            let priv_bytes = decode_private_key(&hex)?;
            let priv_key = StaticSecret::from(priv_bytes);
            let pub_key: PublicKey = (&priv_key).into();
            // Drop the StaticSecret as soon as we have the public; it
            // goes out of scope at function exit but the explicit
            // drop here keeps the lifetime narrow.
            drop(priv_key);
            let device_pub = DevicePublicKey::from(pub_key);
            cache_public_key(handle, &device_pub)?;
            Ok(device_pub)
        }
        Err(KeychainError::NotFound) => {
            // Fresh generation path.
            let priv_key = StaticSecret::random_from_rng(OsRng);
            let pub_key: PublicKey = (&priv_key).into();
            // Persist the private side as hex; we never log it.
            let priv_hex = encode_private_key(priv_key.as_bytes());
            keychain.set_private_key_hex(&priv_hex)?;
            // Drop the StaticSecret right after we have what we need.
            drop(priv_key);
            let device_pub = DevicePublicKey::from(pub_key);
            cache_public_key(handle, &device_pub)?;
            Ok(device_pub)
        }
        Err(other) => Err(KeypairError::Keychain(other)),
    }
}

/// Read the cached public key from `app_settings`, falling back to
/// `None` when the setting is absent or malformed. Useful for the
/// `backup_status` command that wants to surface "key present" without
/// touching the keychain (the public key is harmless to read on every
/// call).
#[must_use]
pub fn read_cached_public_key(handle: &IndexHandle) -> Option<DevicePublicKey> {
    let raw = read_setting_raw(handle, KEY_BACKUP_PUBLIC_KEY)
        .ok()
        .flatten();
    let Some(serde_json::Value::String(hex)) = raw else {
        return None;
    };
    DevicePublicKey::from_hex(&hex)
}

fn cache_public_key(
    handle: &IndexHandle,
    pub_key: &DevicePublicKey,
) -> Result<(), crate::index::IndexError> {
    let value = serde_json::Value::String(pub_key.to_hex());
    write_setting_raw(handle, KEY_BACKUP_PUBLIC_KEY, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::keychain::test_support::InMemoryKeychain;
    use crate::backup::keychain::Keychain;

    #[test]
    fn first_call_generates_keypair_and_caches_public() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let kc = InMemoryKeychain::new();
        // No prior key.
        assert!(read_cached_public_key(&handle).is_none());

        let pub_key = ensure_keypair_with(&handle, &kc).expect("ensure");
        // Public is now cached in app_settings.
        let cached = read_cached_public_key(&handle).expect("cached");
        assert_eq!(cached.0, pub_key.0);

        // Keychain has the matching private (we round-trip the hex
        // through the public-key derivation to confirm).
        let priv_hex = kc.get_private_key_hex().expect("kc has key");
        let priv_bytes = decode_private_key(&priv_hex).expect("decode");
        let priv_key = StaticSecret::from(priv_bytes);
        let derived_pub: PublicKey = (&priv_key).into();
        assert_eq!(derived_pub.to_bytes(), pub_key.0);
    }

    #[test]
    fn second_call_is_idempotent() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let kc = InMemoryKeychain::new();

        let first = ensure_keypair_with(&handle, &kc).expect("ensure 1");
        let second = ensure_keypair_with(&handle, &kc).expect("ensure 2");
        assert_eq!(first.0, second.0, "two calls must return the same public");

        // Keychain still has the original private; second call did
        // NOT overwrite it.
        assert!(kc.get_private_key_hex().is_ok());
    }

    #[test]
    fn cache_recovers_when_app_settings_lost() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let kc = InMemoryKeychain::new();

        let original = ensure_keypair_with(&handle, &kc).expect("ensure");

        // Simulate a "reset_index" that wipes app_settings without
        // touching the keychain.
        handle
            .write(|c| {
                c.execute("DELETE FROM app_settings", [])
                    .map(|_| ())
                    .map_err(Into::into)
            })
            .expect("wipe settings");
        assert!(read_cached_public_key(&handle).is_none());

        // Next call must rebuild the cache from the keychain rather
        // than generating a fresh keypair.
        let recovered = ensure_keypair_with(&handle, &kc).expect("ensure 2");
        assert_eq!(
            recovered.0, original.0,
            "must reuse the keychain entry, not regenerate",
        );
        assert!(read_cached_public_key(&handle).is_some());
    }

    #[test]
    fn malformed_keychain_entry_surfaces_typed_error() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let kc = InMemoryKeychain::new();
        kc.set_private_key_hex("not actual hex value of 64 chars at all---")
            .expect("set garbage");
        let err = ensure_keypair_with(&handle, &kc).expect_err("must fail");
        match err {
            KeypairError::Keychain(KeychainError::MalformedEntry) => {}
            other => panic!("expected MalformedEntry, got {other:?}"),
        }
    }

    /// Real-keychain integration test. Skipped by default because it
    /// mutates the actual macOS Keychain; opt in via
    /// `ASEYE_TEST_KEYCHAIN=1 cargo test ensure_keypair_idempotent`.
    #[test]
    fn ensure_keypair_idempotent_real_keychain() {
        if std::env::var("ASEYE_TEST_KEYCHAIN").as_deref() != Ok("1") {
            return;
        }
        let handle = IndexHandle::open_in_memory().expect("open");
        let first = ensure_keypair(&handle).expect("ensure 1");
        let second = ensure_keypair(&handle).expect("ensure 2");
        assert_eq!(
            first.0, second.0,
            "real keychain must round-trip the same public",
        );
    }
}
