//! Envelope encryption: AES-256-GCM data + X25519/HKDF wrapped DEK.
//!
//! Mirrors `docs/15-backup-and-restore.md` section 15.3. The contract
//! is:
//!
//! ```text
//! plaintext bytes
//!     |
//!     +-- generate random 32-byte DEK
//!     +-- AES-256-GCM(DEK, plaintext, nonce_data) -> ciphertext + data_tag
//!     +-- generate ephemeral X25519 keypair (eph_priv, eph_pub)
//!     +-- shared = ECDH(eph_priv, device_pub)
//!     +-- wrap_key = HKDF-SHA256(shared,
//!                                info = "aseye-backup-wrap-v1",
//!                                salt = device_pub || eph_pub)
//!     +-- AES-256-GCM(wrap_key, DEK, nonce_wrap) -> wrapped_DEK + wrap_tag
//!     |
//!     +-> blob = magic(4) | version(4) | eph_pub(32)
//!               | nonce_wrap(12) | wrapped_DEK(32) | wrap_tag(16)
//!               | nonce_data(12) | ciphertext(N) | data_tag(16)
//! ```
//!
//! Keys never leave this module unencrypted - the `wrap_key` is derived
//! per-blob and discarded after the AES-GCM call returns. The DEK is
//! generated per-blob too. Two files with identical plaintext produce
//! distinct ciphertext because the DEK + ephemeral keypair are fresh.
//!
//! ### Why this shape
//!
//! HPKE (RFC 9180) gives the same forward secrecy guarantee but pulls
//! in a much heavier crate footprint and a wider interop surface than
//! we need. The minimal subset above gives us:
//!
//! * forward secrecy at the wrap layer (compromising the device key
//!   tomorrow does not retroactively decrypt today's blobs unless the
//!   attacker also has the storage backend),
//! * deterministic blob layout that an S3 / R2 backend can put-byte-
//!   for-byte without any re-framing,
//! * a single hard error surface (`EnvelopeError`) the IPC layer maps
//!   to a per-component `BackupErrorKind`.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize_compat::DropZeros;

/// Magic bytes identifying an All-Seeing-eye Backup Volume blob. Four
/// ASCII bytes so a hex dump is human-readable and a stray ".bin" file
/// can be triaged at-a-glance ("ASBV...").
pub const MAGIC: &[u8; 4] = b"ASBV";

/// Current blob format version. The loader rejects anything else with
/// a clear error rather than guessing - format breakage is a bigger
/// deal than a missed restore.
pub const BLOB_VERSION: u32 = 1;

/// Length of the fixed-size header that precedes the ciphertext.
///
/// 4 (magic) + 4 (version) + 32 (`eph_pub`) + 12 (`nonce_wrap`)
/// + 32 (`wrapped_DEK`) + 16 (`wrap_tag`) + 12 (`nonce_data`) = 112.
pub const BLOB_HEADER_LEN: usize = 4 + 4 + 32 + 12 + 32 + 16 + 12;

/// Length of the AES-GCM authentication tag appended to the ciphertext.
pub const DATA_TAG_LEN: usize = 16;

/// HKDF info string. Bumping this string is equivalent to bumping
/// [`BLOB_VERSION`] - older blobs derived against the previous string
/// will fail to decrypt.
const HKDF_INFO: &[u8] = b"aseye-backup-wrap-v1";

/// Newtype around the 32-byte X25519 device public key. Public keys
/// are safe to copy + log (they are not secret) so the wrapping is
/// purely about preventing accidental confusion with the private side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevicePublicKey(pub [u8; 32]);

impl DevicePublicKey {
    /// Build from a hex string (the form we cache in
    /// `app_settings.backupPublicKey`). Returns `None` if the string
    /// is not 64 hex characters.
    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        let bytes = hex_decode(hex)?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(Self(arr))
    }

    /// Hex-encode the key for storage in `app_settings`. Takes
    /// `self` by value because the underlying type is `Copy` and
    /// clippy prefers value receivers for `to_*` on `Copy` types.
    #[must_use]
    pub fn to_hex(self) -> String {
        hex_encode(&self.0)
    }

    /// Borrow as a slice for ECDH consumption.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<PublicKey> for DevicePublicKey {
    fn from(pk: PublicKey) -> Self {
        Self(pk.to_bytes())
    }
}

/// Errors that can come out of [`encrypt_blob`] / [`decrypt_blob`].
///
/// `Aead` failures collapse to a single variant by design - exposing
/// the difference between "wrong DEK" and "tampered tag" would help an
/// attacker mount a side-channel; AES-GCM is supposed to fail closed.
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("blob is shorter than the required header ({needed} bytes, got {got})")]
    Truncated { needed: usize, got: usize },

    #[error("blob magic mismatch: expected {expected:?}, got {actual:?}")]
    BadMagic { expected: [u8; 4], actual: [u8; 4] },

    #[error("unsupported blob version {version}; this build only supports v{supported}")]
    UnsupportedVersion { version: u32, supported: u32 },

    /// AES-GCM failed at the wrap stage (wrong device key, tampered
    /// `wrap_tag`, or tampered ephemeral public key).
    #[error("failed to unwrap data encryption key (auth tag mismatch)")]
    UnwrapFailed,

    /// AES-GCM failed at the data stage (tampered ciphertext or tag).
    #[error("failed to decrypt ciphertext (auth tag mismatch)")]
    DecryptFailed,

    /// AES-GCM failed during encrypt - extremely rare, would mean the
    /// underlying RNG ran out or the input length overflowed.
    #[error("AEAD encryption failed: {0}")]
    EncryptFailed(String),

    /// HKDF expansion failed (caller asked for too many bytes).
    #[error("HKDF expand failed: {0}")]
    HkdfFailed(String),
}

/// Encrypt `plaintext` against the device public key.
///
/// Returns the full self-describing blob bytes - magic, version,
/// ephemeral pub, wrapped DEK + tag, ciphertext + tag - that
/// [`decrypt_blob`] can round-trip.
pub fn encrypt_blob(
    device_pub: &DevicePublicKey,
    plaintext: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    // 1. Random DEK (data encryption key).
    let mut dek = [0u8; 32];
    OsRng.fill_bytes(&mut dek);

    // 2. Random nonces. AES-GCM is a 96-bit nonce AEAD; a 12-byte
    //    random sample collides at ~2^48 messages with the same key.
    //    We use two independent keys (DEK + wrap_key) so even crossing
    //    that count for one would not poison the other.
    let mut nonce_data = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_data);
    let mut nonce_wrap = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_wrap);

    // 3. Encrypt plaintext under DEK -> ciphertext (includes tag).
    let data_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
    let ciphertext_with_tag = data_cipher
        .encrypt(Nonce::from_slice(&nonce_data), plaintext)
        .map_err(|e| EnvelopeError::EncryptFailed(e.to_string()))?;

    // 4. Ephemeral X25519 keypair + ECDH.
    let eph_priv = StaticSecret::random_from_rng(OsRng);
    let eph_pub: PublicKey = (&eph_priv).into();
    let device_pub_curve = PublicKey::from(*device_pub.as_bytes());
    let shared = eph_priv.diffie_hellman(&device_pub_curve);

    // 5. HKDF over the shared secret. Salt binds the wrap to the
    //    specific (device_pub || eph_pub) pair so an attacker cannot
    //    substitute a different ephemeral pub without invalidating the
    //    wrap_tag.
    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(device_pub.as_bytes());
    salt.extend_from_slice(eph_pub.as_bytes());
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut wrap_key = [0u8; 32];
    hkdf.expand(HKDF_INFO, &mut wrap_key)
        .map_err(|e| EnvelopeError::HkdfFailed(e.to_string()))?;

    // 6. Wrap the DEK under wrap_key. AES-GCM(wrap_key, DEK) gives
    //    32 wrapped bytes + 16-byte tag = 48 bytes total.
    let wrap_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrap_key));
    let wrapped_with_tag = wrap_cipher
        .encrypt(Nonce::from_slice(&nonce_wrap), dek.as_slice())
        .map_err(|e| EnvelopeError::EncryptFailed(e.to_string()))?;

    // Drop sensitive material as soon as we no longer need it.
    wrap_key.drop_zeros();
    dek.drop_zeros();

    // 7. Compose the blob exactly as the spec lays it out.
    let mut blob = Vec::with_capacity(BLOB_HEADER_LEN + ciphertext_with_tag.len());
    blob.extend_from_slice(MAGIC);
    blob.extend_from_slice(&BLOB_VERSION.to_le_bytes());
    blob.extend_from_slice(eph_pub.as_bytes());
    blob.extend_from_slice(&nonce_wrap);
    debug_assert_eq!(wrapped_with_tag.len(), 32 + 16);
    blob.extend_from_slice(&wrapped_with_tag[..32]); // wrapped DEK
    blob.extend_from_slice(&wrapped_with_tag[32..]); // wrap tag
    blob.extend_from_slice(&nonce_data);
    blob.extend_from_slice(&ciphertext_with_tag);

    debug_assert_eq!(
        blob.len(),
        BLOB_HEADER_LEN + plaintext.len() + DATA_TAG_LEN,
        "blob length must match header + plaintext + tag",
    );
    Ok(blob)
}

/// Decrypt a blob produced by [`encrypt_blob`] using the device
/// private key.
///
/// `device_priv` is consumed by reference; the caller is expected to
/// fetch it fresh from the keychain immediately before calling and to
/// drop it as soon as the call returns. Long-lived caching of the
/// private key in process memory is explicitly forbidden by the spec
/// (15.2 "Lifecycle").
pub fn decrypt_blob(device_priv: &StaticSecret, blob: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    if blob.len() < BLOB_HEADER_LEN + DATA_TAG_LEN {
        return Err(EnvelopeError::Truncated {
            needed: BLOB_HEADER_LEN + DATA_TAG_LEN,
            got: blob.len(),
        });
    }

    // 1. Validate header.
    let magic: &[u8; 4] = blob[0..4].try_into().expect("4-byte slice");
    if magic != MAGIC {
        return Err(EnvelopeError::BadMagic {
            expected: *MAGIC,
            actual: *magic,
        });
    }

    let version = u32::from_le_bytes(blob[4..8].try_into().expect("4-byte slice"));
    if version != BLOB_VERSION {
        return Err(EnvelopeError::UnsupportedVersion {
            version,
            supported: BLOB_VERSION,
        });
    }

    let eph_pub_bytes: [u8; 32] = blob[8..40].try_into().expect("32-byte slice");
    let eph_pub = PublicKey::from(eph_pub_bytes);
    let nonce_wrap: [u8; 12] = blob[40..52].try_into().expect("12-byte slice");
    let wrapped_dek = &blob[52..84];
    let wrap_tag = &blob[84..100];
    let nonce_data: [u8; 12] = blob[100..112].try_into().expect("12-byte slice");
    let ciphertext_with_tag = &blob[BLOB_HEADER_LEN..];

    // 2. Recompute the wrap key from device_priv x eph_pub.
    let device_pub: PublicKey = device_priv.into();
    let shared = device_priv.diffie_hellman(&eph_pub);

    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(device_pub.as_bytes());
    salt.extend_from_slice(eph_pub.as_bytes());
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut wrap_key = [0u8; 32];
    hkdf.expand(HKDF_INFO, &mut wrap_key)
        .map_err(|e| EnvelopeError::HkdfFailed(e.to_string()))?;

    // 3. Unwrap the DEK. AES-GCM consumes ciphertext|tag concatenated.
    let mut wrap_input = Vec::with_capacity(wrapped_dek.len() + wrap_tag.len());
    wrap_input.extend_from_slice(wrapped_dek);
    wrap_input.extend_from_slice(wrap_tag);
    let wrap_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrap_key));
    let dek_bytes = wrap_cipher
        .decrypt(Nonce::from_slice(&nonce_wrap), wrap_input.as_slice())
        .map_err(|_| EnvelopeError::UnwrapFailed)?;
    wrap_key.drop_zeros();
    if dek_bytes.len() != 32 {
        return Err(EnvelopeError::UnwrapFailed);
    }
    let mut dek = [0u8; 32];
    dek.copy_from_slice(&dek_bytes);

    // 4. Decrypt the data.
    let data_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
    let plaintext = data_cipher
        .decrypt(Nonce::from_slice(&nonce_data), ciphertext_with_tag)
        .map_err(|_| EnvelopeError::DecryptFailed)?;
    dek.drop_zeros();

    Ok(plaintext)
}

// --- helpers --------------------------------------------------------

/// Internal helper - encode bytes as lowercase hex.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Two-char hex per byte, lowercase, no allocation per push.
        let hi = HEX_LOWER[((b >> 4) & 0xF) as usize];
        let lo = HEX_LOWER[(b & 0xF) as usize];
        s.push(hi);
        s.push(lo);
    }
    s
}

/// Internal helper - decode lowercase or uppercase hex; returns `None`
/// when the string is odd-length or contains non-hex characters.
#[must_use]
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

const HEX_LOWER: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Tiny shim trait so `[u8; N]` and `Vec<u8>` carry a uniform "drop
/// zeros" call without pulling in the full `zeroize` crate. The
/// production-grade hardening would use `zeroize::Zeroize`; this
/// minimal version is enough for the spec's "do not cache the private
/// key in process memory" rule because we only use it on stack-local
/// buffers that go out of scope at function return.
mod zeroize_compat {
    pub trait DropZeros {
        fn drop_zeros(&mut self);
    }

    impl DropZeros for [u8; 32] {
        #[inline]
        fn drop_zeros(&mut self) {
            // Volatile write so the compiler cannot prove the writes
            // are dead and elide them. `core::ptr::write_volatile`
            // is the documented mechanism for this; loops are also
            // commonly used but the compiler is allowed to prove a
            // loop dead in the absence of the volatile annotation.
            for byte in self.iter_mut() {
                // SAFETY: writing to a stack-local mutable byte through
                // a non-aliased pointer is well-defined; volatile only
                // affects optimisation, not validity. The lint forbids
                // `unsafe` workspace-wide, so we use the safe fallback
                // below: a plain assignment in a `core::hint::black_box`
                // wrapper. `black_box` is documented as preventing
                // dead-code elimination of the value passed through it.
                *byte = 0;
                std::hint::black_box(*byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::StaticSecret;

    fn fresh_keypair() -> (StaticSecret, DevicePublicKey) {
        let priv_key = StaticSecret::random_from_rng(OsRng);
        let pub_key: PublicKey = (&priv_key).into();
        (priv_key, DevicePublicKey::from(pub_key))
    }

    /// Round-trip: encrypt + decrypt with the same device keypair must
    /// recover the original bytes byte-for-byte.
    #[test]
    fn encrypt_then_decrypt_roundtrip() {
        let (priv_key, pub_key) = fresh_keypair();
        let plaintext = b"the quick brown fox jumps over the lazy dog";
        let blob = encrypt_blob(&pub_key, plaintext).expect("encrypt");
        let decrypted = decrypt_blob(&priv_key, &blob).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    /// The DEK + ephemeral keypair are fresh per blob, so two
    /// encryptions of the same plaintext yield distinct ciphertext.
    /// Forward-secrecy at the wrap layer depends on this.
    #[test]
    fn two_encryptions_diverge() {
        let (_, pub_key) = fresh_keypair();
        let plaintext = b"identical content";
        let a = encrypt_blob(&pub_key, plaintext).expect("a");
        let b = encrypt_blob(&pub_key, plaintext).expect("b");
        assert_ne!(a, b, "encryptions of identical content must differ");
    }

    /// Blob layout: header is exactly 112 bytes; total length is
    /// header + plaintext + 16-byte data tag.
    #[test]
    fn blob_layout_is_exact() {
        let (_, pub_key) = fresh_keypair();
        let plaintext = vec![0u8; 1000];
        let blob = encrypt_blob(&pub_key, &plaintext).expect("encrypt");
        assert_eq!(blob.len(), BLOB_HEADER_LEN + plaintext.len() + DATA_TAG_LEN);
        assert_eq!(&blob[0..4], MAGIC);
        assert_eq!(
            u32::from_le_bytes(blob[4..8].try_into().unwrap()),
            BLOB_VERSION
        );
    }

    /// Tamper with one ciphertext byte; decrypt must fail closed
    /// rather than returning corrupted plaintext.
    #[test]
    fn tamper_with_ciphertext_fails_closed() {
        let (priv_key, pub_key) = fresh_keypair();
        let plaintext = b"do not flip a bit on me";
        let mut blob = encrypt_blob(&pub_key, plaintext).expect("encrypt");
        // Flip a byte well inside the ciphertext segment.
        let target = BLOB_HEADER_LEN + 5;
        blob[target] ^= 0xFF;
        let err = decrypt_blob(&priv_key, &blob).expect_err("must fail");
        assert!(matches!(err, EnvelopeError::DecryptFailed));
    }

    /// Tamper with one byte of the wrapped DEK; decrypt must fail
    /// closed at the unwrap stage with the dedicated variant.
    #[test]
    fn tamper_with_wrap_fails_closed() {
        let (priv_key, pub_key) = fresh_keypair();
        let plaintext = b"hands off my wrap";
        let mut blob = encrypt_blob(&pub_key, plaintext).expect("encrypt");
        // Wrapped DEK lives at offset 52..84.
        blob[52] ^= 0x01;
        let err = decrypt_blob(&priv_key, &blob).expect_err("must fail");
        assert!(matches!(err, EnvelopeError::UnwrapFailed));
    }

    /// Tamper with the wrap tag specifically; same closed failure.
    #[test]
    fn tamper_with_wrap_tag_fails_closed() {
        let (priv_key, pub_key) = fresh_keypair();
        let plaintext = b"tag is sacred";
        let mut blob = encrypt_blob(&pub_key, plaintext).expect("encrypt");
        // Wrap tag lives at offset 84..100.
        blob[84] ^= 0x40;
        let err = decrypt_blob(&priv_key, &blob).expect_err("must fail");
        assert!(matches!(err, EnvelopeError::UnwrapFailed));
    }

    /// Decrypting with a different private key must fail at the
    /// unwrap stage. This is the central security property: the
    /// storage backend can read every byte of the blob and still
    /// cannot recover the plaintext.
    #[test]
    fn wrong_device_key_fails_closed() {
        let (_correct_priv, pub_key) = fresh_keypair();
        let (other_priv, _) = fresh_keypair();
        let plaintext = b"private to one device";
        let blob = encrypt_blob(&pub_key, plaintext).expect("encrypt");
        let err = decrypt_blob(&other_priv, &blob).expect_err("must fail");
        assert!(matches!(err, EnvelopeError::UnwrapFailed));
    }

    /// Magic mismatch is caught as a typed error, not a generic
    /// decrypt failure - we want the UI to render "this is not an
    /// aseye blob" distinct from "this blob did not decrypt".
    #[test]
    fn bad_magic_rejected() {
        let (priv_key, _pub_key) = fresh_keypair();
        let mut blob = vec![0u8; BLOB_HEADER_LEN + DATA_TAG_LEN];
        blob[0..4].copy_from_slice(b"NOPE");
        let err = decrypt_blob(&priv_key, &blob).expect_err("must fail");
        assert!(matches!(err, EnvelopeError::BadMagic { .. }));
    }

    /// A future format will bump version; today's binary refuses to
    /// guess at the layout.
    #[test]
    fn unsupported_version_rejected() {
        let (priv_key, pub_key) = fresh_keypair();
        let mut blob = encrypt_blob(&pub_key, b"x").expect("encrypt");
        blob[4..8].copy_from_slice(&999u32.to_le_bytes());
        let err = decrypt_blob(&priv_key, &blob).expect_err("must fail");
        match err {
            EnvelopeError::UnsupportedVersion { version, supported } => {
                assert_eq!(version, 999);
                assert_eq!(supported, BLOB_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    /// Truncated blobs are flagged before any crypto fires.
    #[test]
    fn truncated_blob_rejected() {
        let (priv_key, _) = fresh_keypair();
        let blob = vec![0u8; 10];
        let err = decrypt_blob(&priv_key, &blob).expect_err("must fail");
        assert!(matches!(err, EnvelopeError::Truncated { .. }));
    }

    /// Empty plaintext is a valid input; decrypt round-trips an
    /// empty Vec.
    #[test]
    fn empty_plaintext_roundtrips() {
        let (priv_key, pub_key) = fresh_keypair();
        let blob = encrypt_blob(&pub_key, b"").expect("encrypt");
        assert_eq!(blob.len(), BLOB_HEADER_LEN + DATA_TAG_LEN);
        let decrypted = decrypt_blob(&priv_key, &blob).expect("decrypt");
        assert!(decrypted.is_empty());
    }

    /// Larger payload (1 MiB) round-trips correctly. We do not
    /// exercise multi-GB payloads in the unit suite - the integration
    /// test covers the realistic shape of indexed components.
    #[test]
    fn large_plaintext_roundtrips() {
        let (priv_key, pub_key) = fresh_keypair();
        let plaintext = vec![0xCDu8; 1024 * 1024];
        let blob = encrypt_blob(&pub_key, &plaintext).expect("encrypt");
        let decrypted = decrypt_blob(&priv_key, &blob).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    /// Hex helpers round-trip and reject malformed strings.
    #[test]
    fn hex_helpers_round_trip() {
        let original = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x01, 0x02];
        let encoded = hex_encode(&original);
        assert_eq!(encoded, "deadbeef0102");
        let decoded = hex_decode(&encoded).expect("decode");
        assert_eq!(decoded, original);

        assert!(hex_decode("oops").is_none());
        assert!(hex_decode("abc").is_none()); // odd length
    }

    /// `DevicePublicKey::from_hex` accepts the wire format produced
    /// by `to_hex` and rejects garbage cleanly.
    #[test]
    fn device_public_key_hex_round_trip() {
        let (_, pub_key) = fresh_keypair();
        let hex = pub_key.to_hex();
        assert_eq!(hex.len(), 64);
        let parsed = DevicePublicKey::from_hex(&hex).expect("parse");
        assert_eq!(parsed.0, pub_key.0);
        assert!(DevicePublicKey::from_hex("deadbeef").is_none());
    }
}
