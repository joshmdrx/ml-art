//! T-086 — bearer tokens for draft-profile preview links.
//!
//! A preview link (`/preview/<token>`) is the only credential needed to
//! view, claim or remove a draft, so the token must be unguessable and
//! must never be recoverable from the database:
//!
//!   - `generate()` returns 64 lowercase hex chars built from two v4
//!     UUIDs (~244 bits of OS randomness via `getrandom`). Plenty for a
//!     link that also expires and is revocable.
//!   - `hash()` is the SHA-256 hex digest stored in
//!     `artist_preview_links.token_hash`. Lookups hash the presented
//!     token and compare in SQL; the plaintext is only ever returned
//!     once, to the admin who minted it.
//!
//! `looks_valid()` lets handlers reject obviously malformed tokens with
//! a 404 before touching the database.

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Length of a generated token, in characters.
pub const TOKEN_LEN: usize = 64;

/// Mint a fresh plaintext token.
pub fn generate() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// SHA-256 hex digest of a token, as stored in the database.
pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Cheap shape check: exactly `TOKEN_LEN` lowercase hex characters.
pub fn looks_valid(token: &str) -> bool {
    token.len() == TOKEN_LEN
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_well_formed_and_distinct() {
        let a = generate();
        let b = generate();
        assert!(looks_valid(&a), "{a}");
        assert!(looks_valid(&b), "{b}");
        assert_ne!(a, b);
    }

    #[test]
    fn hash_is_stable_and_not_the_token() {
        let t = generate();
        assert_eq!(hash(&t), hash(&t));
        assert_ne!(hash(&t), t);
        assert_eq!(hash(&t).len(), 64);
    }

    #[test]
    fn looks_valid_rejects_bad_shapes() {
        assert!(!looks_valid(""));
        assert!(!looks_valid("abc"));
        assert!(!looks_valid(&"A".repeat(TOKEN_LEN)));
        assert!(!looks_valid(&"g".repeat(TOKEN_LEN)));
        assert!(!looks_valid(&"a".repeat(TOKEN_LEN + 1)));
    }
}
