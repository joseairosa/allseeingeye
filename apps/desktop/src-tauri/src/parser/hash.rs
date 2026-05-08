//! Content hashing helper.
//!
//! Per `docs/05-data-architecture.md`, every parsed component carries a
//! SHA-256 hex digest of its raw bytes. The hash drives change detection
//! (compare on-disk hash vs indexed hash) and is the cheapest fingerprint
//! we can compute that survives byte-identical re-saves.
//!
//! Lowercase hex is the canonical wire format - it matches `git`'s
//! convention and is unambiguous when round-tripped through JSON.

use sha2::{Digest, Sha256};

/// SHA-256 digest of `bytes`, encoded as lowercase hex.
///
/// Length is always 64 ASCII characters. Empty input yields the standard
/// `e3b0c44...b855` sentinel from FIPS 180-4 §B.1.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();

    // Manual lowercase-hex encode: avoids pulling `hex` as a new
    // dep when this is the only call site. 64 nibbles = 64 ASCII
    // chars; pre-allocate to skip the growth dance.
    let mut out = String::with_capacity(64);
    for byte in digest {
        // `write!` on a String can't fail; `{byte:02x}` always emits
        // two lowercase hex digits per byte.
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn sha256_hex_known_vector_empty() {
        // FIPS 180-4 test vector: SHA-256("") =
        // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_known_vector_abc() {
        // FIPS 180-4 test vector: SHA-256("abc") =
        // ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_hex_is_64_chars() {
        // Defensive: verify the output length contract for arbitrary
        // input. Anything other than 64 lowercase hex chars would
        // break callers that store the hash in a `TEXT(64)` column.
        let h = sha256_hex(b"the quick brown fox");
        assert_eq!(h.len(), 64);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }
}
