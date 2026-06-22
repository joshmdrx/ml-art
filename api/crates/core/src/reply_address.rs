//! T-054 — Tokenised reply-to addresses for email-stitched inquiry threads.
//!
//! When an artist replies to an inquiry, the outbound email's `Reply-To`
//! is a per-inquiry address of the form:
//!
//! ```text
//! r-<inquiry_id_simple>-<hmac>@reply.<domain>
//! ```
//!
//! `<inquiry_id_simple>` is the inquiry UUID in hyphen-free lowercase hex
//! (32 chars); `<hmac>` is the first 10 bytes of
//! `HMAC-SHA256(secret, inquiry_id_simple)`, hex-encoded (20 chars). The
//! whole local part is `2 + 32 + 1 + 20 = 55` chars — under RFC 5321's
//! 64-char limit, which a full JWT (as used for unsubscribe tokens in
//! [`crate::notifications`]) would blow past.
//!
//! The HMAC authenticates the inquiry id so a stranger can't synthesise a
//! valid reply address for an arbitrary inquiry. The secret is the same
//! `anon_cookie_secret` reused across the app's HMAC needs (see
//! [`crate::notifications`] for the single-secret rationale).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Local-part prefix. Distinguishes reply addresses from any other mail
/// the routing domain might receive.
const PREFIX: &str = "r";

/// Bytes of the HMAC kept in the address. 10 bytes = 80 bits — far more
/// than enough to make forging a valid address for a target inquiry
/// infeasible, while keeping the local part well under 64 chars.
const MAC_BYTES: usize = 10;

/// Build the full tokenised reply-to address for `inquiry_id` under
/// `domain` (e.g. `reply.wander.gallery`).
pub fn mint(inquiry_id: Uuid, domain: &str, secret: &[u8]) -> String {
    let id = inquiry_id.simple().to_string();
    let mac = tag_hex(&id, secret);
    format!("{PREFIX}-{id}-{mac}@{domain}")
}

/// Parse + verify a reply address (or bare local part) and return the
/// `inquiry_id` it authenticates. Returns `None` for anything malformed,
/// or whose HMAC doesn't match — i.e. a forged or tampered address.
pub fn verify(address_or_local: &str, secret: &[u8]) -> Option<Uuid> {
    // Accept either the full address or just the local part.
    let local = address_or_local.split('@').next().unwrap_or("");
    // Exactly three '-'-separated parts: prefix, uuid, mac. The simple
    // uuid + hex mac contain no '-', so this split is unambiguous.
    let mut parts = local.splitn(3, '-');
    let prefix = parts.next()?;
    let id_str = parts.next()?;
    let mac_hex = parts.next()?;
    if prefix != PREFIX {
        return None;
    }
    // Canonicalise via parse → simple, so the recomputed MAC matches
    // `mint` regardless of the incoming hyphenation/case.
    let inquiry_id = Uuid::parse_str(id_str).ok()?;
    let canonical = inquiry_id.simple().to_string();

    let tag = hex::decode(mac_hex).ok()?;
    if tag.len() != MAC_BYTES {
        return None;
    }
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(canonical.as_bytes());
    // Constant-time comparison of the truncated tag.
    mac.verify_truncated_left(&tag).ok()?;
    Some(inquiry_id)
}

/// Hex-encode the first `MAC_BYTES` of `HMAC-SHA256(secret, msg)`.
fn tag_hex(msg: &str, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(msg.as_bytes());
    let bytes = mac.finalize().into_bytes();
    hex::encode(&bytes[..MAC_BYTES])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-do-not-use-in-prod";
    const DOMAIN: &str = "reply.test.example.com";

    #[test]
    fn round_trip_returns_inquiry_id() {
        let id = Uuid::new_v4();
        let addr = mint(id, DOMAIN, SECRET);
        assert_eq!(verify(&addr, SECRET), Some(id));
    }

    #[test]
    fn local_part_within_rfc5321_limit() {
        let id = Uuid::new_v4();
        let addr = mint(id, DOMAIN, SECRET);
        let local = addr.split('@').next().unwrap();
        assert!(
            local.len() <= 64,
            "local part {} chars exceeds RFC 5321 limit: {local}",
            local.len()
        );
        // Pin the exact shape so a future change can't silently regress.
        assert_eq!(local.len(), 55);
    }

    #[test]
    fn verify_accepts_bare_local_part() {
        let id = Uuid::new_v4();
        let addr = mint(id, DOMAIN, SECRET);
        let local = addr.split('@').next().unwrap();
        assert_eq!(verify(local, SECRET), Some(id));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let id = Uuid::new_v4();
        let addr = mint(id, DOMAIN, SECRET);
        assert_eq!(verify(&addr, b"a-different-secret"), None);
    }

    #[test]
    fn tampered_mac_is_rejected() {
        let id = Uuid::new_v4();
        let addr = mint(id, DOMAIN, SECRET);
        // Flip the final hex digit of the MAC.
        let mut bytes = addr.into_bytes();
        let at = bytes.iter().position(|&b| b == b'@').unwrap();
        let last = at - 1;
        bytes[last] = if bytes[last] == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(bytes).unwrap();
        assert_eq!(verify(&tampered, SECRET), None);
    }

    #[test]
    fn mac_from_another_inquiry_is_rejected() {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let addr_a = mint(id_a, DOMAIN, SECRET);
        let addr_b = mint(id_b, DOMAIN, SECRET);
        // Splice id_b's uuid into id_a's address (keeping id_a's MAC).
        let mac_a = addr_a.split('-').nth(2).unwrap().split('@').next().unwrap();
        let id_b_simple = id_b.simple().to_string();
        let forged = format!("r-{id_b_simple}-{mac_a}@{DOMAIN}");
        assert_eq!(verify(&forged, SECRET), None);
        // Sanity: each address still verifies to its own id.
        assert_eq!(verify(&addr_b, SECRET), Some(id_b));
    }

    #[test]
    fn malformed_inputs_are_rejected() {
        for bad in [
            "",
            "nope",
            "r-not-a-uuid",
            "x-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-0011223344556677889900",
            "@reply.test.example.com",
        ] {
            assert_eq!(verify(bad, SECRET), None, "should reject: {bad:?}");
        }
    }
}
