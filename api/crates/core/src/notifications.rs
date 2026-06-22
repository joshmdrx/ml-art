//! T-068 — Email-notification preferences + unsubscribe tokens.
//!
//! Shared by every notification-emitting surface (T-052b new-work
//! digest, T-059 saved-search alerts, T-060 Discover Weekly, future
//! artist-side new-follower + new-inquiry digests). Every send routes
//! through [`user_wants`]; every email footer carries an [`mint_unsubscribe_token`]
//! link verified by [`verify_unsubscribe_token`].
//!
//! Two kinds of email exist in the system:
//!
//! - **Transactional** — inquiry verification, artist reply to inquirer,
//!   etc. Sent regardless of preferences (the user took an action;
//!   we're confirming or completing it). Legally OK under CAN-SPAM /
//!   CASL / GDPR; matches user expectation.
//!
//! - **Notification** — automated, can-be-suppressed. Each carries an
//!   unsubscribe link + `List-Unsubscribe` header so Gmail/Outlook can
//!   show their built-in one-click UI (and our sender reputation
//!   benefits).
//!
//! The split is encoded in [`NotificationKind::is_transactional`]; the
//! `user_wants` helper short-circuits true for transactional kinds so
//! callers never need to know which is which.

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

/// Every email surface we send maps to one of these. Adding a new
/// notification feature is one variant + a row in the settings UI's
/// `KNOWN_KINDS` list (the API derives its `kinds` map from the enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// Transactional — verification email for an anonymous inquiry.
    InquiryVerification,
    /// Transactional — artist's reply forwarded to the inquirer.
    InquiryReply,
    /// Notification — daily digest of new works from followed artists
    /// (T-052b).
    NewWorksDigest,
}

impl NotificationKind {
    /// Transactional emails bypass the master switch + per-kind
    /// preferences. They're a direct response to a user action and
    /// suppressing them would break the product.
    pub fn is_transactional(self) -> bool {
        matches!(self, Self::InquiryVerification | Self::InquiryReply)
    }

    /// Wire-format / DB string. Stable; do not change without a
    /// migration to rewrite existing `notification_preferences.kind`
    /// values.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InquiryVerification => "inquiry_verification",
            Self::InquiryReply => "inquiry_reply",
            Self::NewWorksDigest => "new_works_digest",
        }
    }

    /// User-facing kinds shown in the settings UI. Transactional kinds
    /// are excluded — there's no toggle for them.
    pub fn user_facing() -> &'static [NotificationKind] {
        &[Self::NewWorksDigest]
    }

    /// Short label for the settings UI. Title-case English.
    pub fn label(self) -> &'static str {
        match self {
            Self::InquiryVerification => "Inquiry verification",
            Self::InquiryReply => "Artist reply",
            Self::NewWorksDigest => "New work from artists you follow",
        }
    }

    /// One-sentence description for the settings UI.
    pub fn description(self) -> &'static str {
        match self {
            Self::InquiryVerification => "Required to confirm inquiries you send.",
            Self::InquiryReply => "When an artist replies to your inquiry.",
            Self::NewWorksDigest => {
                "A daily summary — only sent on days when at least one artist you follow publishes new work."
            }
        }
    }
}

impl fmt::Display for NotificationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NotificationKind {
    type Err = UnknownKind;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inquiry_verification" => Ok(Self::InquiryVerification),
            "inquiry_reply" => Ok(Self::InquiryReply),
            "new_works_digest" => Ok(Self::NewWorksDigest),
            _ => Err(UnknownKind),
        }
    }
}

#[derive(Debug)]
pub struct UnknownKind;

impl fmt::Display for UnknownKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown notification kind")
    }
}

impl std::error::Error for UnknownKind {}

// ─────────────────────────────────────────────────────────────────────────────
// user_wants
// ─────────────────────────────────────────────────────────────────────────────

/// Single chokepoint for "should I send this email?" Every
/// notification-emitting handler routes through here.
///
/// Returns `Ok(true)` immediately for transactional kinds — they
/// don't consult preferences. For everything else: the master kill
/// switch wins (if `users.global_email_notifications_enabled = false`,
/// returns false regardless of per-kind state), then a per-kind row
/// (if any) wins, defaulting to true when no row exists.
pub async fn user_wants(
    pool: &PgPool,
    user_id: Uuid,
    kind: NotificationKind,
) -> Result<bool, sqlx::Error> {
    if kind.is_transactional() {
        return Ok(true);
    }

    let global: Option<bool> =
        sqlx::query_scalar("SELECT global_email_notifications_enabled FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    // Unknown user → don't send. Caller is the one with a bad id.
    let Some(global) = global else {
        return Ok(false);
    };
    if !global {
        return Ok(false);
    }

    let per_kind: Option<bool> = sqlx::query_scalar(
        "SELECT enabled FROM notification_preferences WHERE user_id = $1 AND kind = $2",
    )
    .bind(user_id)
    .bind(kind.as_str())
    .fetch_optional(pool)
    .await?;
    Ok(per_kind.unwrap_or(true))
}

// ─────────────────────────────────────────────────────────────────────────────
// Unsubscribe tokens
// ─────────────────────────────────────────────────────────────────────────────

/// 90 days — long enough that a user clicking the link a month after
/// the email still works; short enough that a leaked link doesn't last
/// forever. Tokens are single-purpose (one user, one kind) so even a
/// long TTL can only flip one preference.
const TOKEN_TTL_DAYS: i64 = 90;

#[derive(Debug, Serialize, Deserialize)]
struct TokenClaims {
    /// User id whose preference we'd flip.
    sub: Uuid,
    /// Which preference. Stored as the snake_case wire form.
    kind: String,
    /// Expiry — unix seconds. jsonwebtoken validates this automatically.
    exp: i64,
}

#[derive(Debug, Error)]
pub enum UnsubscribeError {
    #[error("token is malformed or signed with the wrong key")]
    Invalid,
    #[error("token expired")]
    Expired,
    #[error("token refers to a notification kind we don't recognise")]
    UnknownKind,
}

/// Mint a signed unsubscribe token for `(user_id, kind)`. The output
/// is URL-safe (no padding) so it can drop straight into an email
/// footer link or the `List-Unsubscribe` header.
pub fn mint_unsubscribe_token(
    user_id: Uuid,
    kind: NotificationKind,
    secret: &[u8],
) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = TokenClaims {
        sub: user_id,
        kind: kind.as_str().to_string(),
        exp: (Utc::now() + Duration::days(TOKEN_TTL_DAYS)).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
}

/// Construct the public unsubscribe URL a recipient clicks from the
/// email footer (and that mail clients POST to for RFC 8058 one-click).
/// Single source of truth for the `/u/<token>` URL shape so every
/// notification feature uses the same path.
pub fn unsubscribe_url(web_base_url: &str, token: &str) -> String {
    // Trim a trailing slash on the base so we don't end up with `//`.
    let base = web_base_url.trim_end_matches('/');
    format!("{base}/u/{token}")
}

/// Verify a token and return the `(user_id, kind)` it points at.
/// Rejects expired, malformed, or wrong-key tokens. Constant-time
/// signature comparison is handled by the jsonwebtoken crate.
pub fn verify_unsubscribe_token(
    token: &str,
    secret: &[u8],
) -> Result<(Uuid, NotificationKind), UnsubscribeError> {
    let decoded = decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::default(),
    )
    .map_err(|e| {
        use jsonwebtoken::errors::ErrorKind::*;
        match e.kind() {
            ExpiredSignature => UnsubscribeError::Expired,
            _ => UnsubscribeError::Invalid,
        }
    })?;
    let kind = NotificationKind::from_str(&decoded.claims.kind)
        .map_err(|_| UnsubscribeError::UnknownKind)?;
    Ok((decoded.claims.sub, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-do-not-use-in-prod";

    #[test]
    fn kind_string_roundtrip() {
        for kind in [
            NotificationKind::InquiryVerification,
            NotificationKind::InquiryReply,
            NotificationKind::NewWorksDigest,
        ] {
            let s = kind.as_str();
            let parsed = NotificationKind::from_str(s).unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn token_roundtrip_returns_user_id_and_kind() {
        let user_id = Uuid::new_v4();
        let token =
            mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, SECRET).unwrap();
        let (got_id, got_kind) = verify_unsubscribe_token(&token, SECRET).unwrap();
        assert_eq!(got_id, user_id);
        assert_eq!(got_kind, NotificationKind::NewWorksDigest);
    }

    #[test]
    fn token_rejected_when_signed_with_different_secret() {
        let user_id = Uuid::new_v4();
        let token =
            mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, SECRET).unwrap();
        let err = verify_unsubscribe_token(&token, b"different-secret").unwrap_err();
        assert!(matches!(err, UnsubscribeError::Invalid));
    }

    #[test]
    fn token_rejected_when_tampered() {
        let user_id = Uuid::new_v4();
        let token =
            mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, SECRET).unwrap();
        // Flip a byte in the middle of the (base64) payload.
        let mut bytes: Vec<u8> = token.into_bytes();
        let mid = bytes.len() / 2;
        bytes[mid] = if bytes[mid] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(bytes).unwrap();
        let err = verify_unsubscribe_token(&tampered, SECRET).unwrap_err();
        assert!(matches!(err, UnsubscribeError::Invalid));
    }

    #[test]
    fn token_with_unknown_kind_is_rejected() {
        // Hand-craft a token claiming a kind we don't know about.
        let claims = TokenClaims {
            sub: Uuid::new_v4(),
            kind: "totally_made_up".to_string(),
            exp: (Utc::now() + Duration::days(1)).timestamp(),
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        let err = verify_unsubscribe_token(&token, SECRET).unwrap_err();
        assert!(matches!(err, UnsubscribeError::UnknownKind));
    }

    #[test]
    fn transactional_kinds_are_marked() {
        assert!(NotificationKind::InquiryVerification.is_transactional());
        assert!(NotificationKind::InquiryReply.is_transactional());
        assert!(!NotificationKind::NewWorksDigest.is_transactional());
    }

    #[test]
    fn user_facing_excludes_transactional() {
        for kind in NotificationKind::user_facing() {
            assert!(
                !kind.is_transactional(),
                "{:?} is transactional but listed as user-facing",
                kind,
            );
        }
    }
}
