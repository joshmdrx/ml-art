//! T-008 — Image moderation client.
//!
//! Wraps the call that decides whether an `artwork_images` row is
//! safe to surface publicly. The handler (see `crate::jobs::handle`
//! → `JobEvent::ArtworkImageModerate`) loads the row, calls
//! `ModerationClient::moderate`, and writes the verdict back to
//! `artwork_images.moderation_status`.
//!
//! Two variants today, same shape as `EmailClient` / `GeocodingClient`:
//!
//! - `Disabled` — local dev + when the prod toggle is off. Auto-approves
//!   with an empty label list. The Real path is a no-op until AWS
//!   Rekognition is wired (deferred — see TODO entry).
//! - `Test` — canned `(s3_key → result)` map, no network. Integration
//!   tests pin the s3_key to a known verdict.
//!
//! Why no `Real` variant yet: the production target is AWS Rekognition
//! `DetectModerationLabels`, but the AWS SDK + IAM setup land with our
//! AWS deploy (deferred). Until then, `from_env()` only ever returns
//! `Disabled`, even when `REKOGNITION_ENABLED=true` — we log a warning
//! so the operator notices, but we don't fabricate a Real client. The
//! gating env var exists so the wire-up is a one-spot change later.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Verdict written back to the database. The third state (`Pending`)
/// is what every new row starts at — the moderation handler only ever
/// flips it to `Approved` or `Rejected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModerationStatus {
    Approved,
    Rejected,
}

impl ModerationStatus {
    /// String form matching the `artwork_images.moderation_status`
    /// CHECK constraint values.
    pub fn as_str(&self) -> &'static str {
        match self {
            ModerationStatus::Approved => "approved",
            ModerationStatus::Rejected => "rejected",
        }
    }
}

/// Outcome of a single `moderate` call. `labels` is the raw list of
/// flagged categories (e.g. Rekognition's "Explicit Nudity",
/// "Violence") — empty on approval, populated on rejection. We keep it
/// around for future "why was this rejected" surfacing; today we only
/// log it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModerationResult {
    pub status: ModerationStatus,
    pub labels: Vec<String>,
}

impl ModerationResult {
    pub fn approved() -> Self {
        Self {
            status: ModerationStatus::Approved,
            labels: Vec::new(),
        }
    }
    pub fn rejected(labels: Vec<String>) -> Self {
        Self {
            status: ModerationStatus::Rejected,
            labels,
        }
    }
}

#[derive(Clone)]
pub struct ModerationClient {
    inner: Arc<Inner>,
}

enum Inner {
    /// Auto-approve. Local dev default and the no-AWS fallback.
    Disabled,
    /// Canned `(s3_key → result)` map. Anything not in the map
    /// returns `approved` (mirroring `Disabled`).
    Test {
        canned: Vec<(String, ModerationResult)>,
    },
}

impl ModerationClient {
    /// Production constructor. When `REKOGNITION_ENABLED=true` is set,
    /// logs a warning that the Real client isn't wired yet — and still
    /// returns `Disabled`. Plumbing for the real client lands with our
    /// AWS deploy.
    pub fn from_env() -> Self {
        let enabled = std::env::var("REKOGNITION_ENABLED")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(|s| s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if enabled {
            tracing::warn!(
                "REKOGNITION_ENABLED=true but the Real moderation client \
                 is not yet wired (auto-approving) — see T-008 follow-up",
            );
        }
        Self::disabled()
    }

    pub fn disabled() -> Self {
        Self {
            inner: Arc::new(Inner::Disabled),
        }
    }

    /// Test constructor — `canned` is matched on the s3_key passed to
    /// `moderate`. Non-matches return `approved` so unrelated images
    /// don't accidentally fail moderation in tests that don't care.
    pub fn for_tests(canned: Vec<(String, ModerationResult)>) -> Self {
        Self {
            inner: Arc::new(Inner::Test { canned }),
        }
    }

    /// `true` when this client will actually call out to a moderation
    /// service. Currently always `false` (no Real variant yet).
    pub fn enabled(&self) -> bool {
        false
    }

    /// Inspect an image. `s3_key` is enough — the moderation provider
    /// fetches from S3 itself (when we wire Real Rekognition); in
    /// `Disabled` / `Test` no IO happens.
    pub async fn moderate(&self, s3_key: &str) -> Result<ModerationResult, ModerationError> {
        match &*self.inner {
            Inner::Disabled => Ok(ModerationResult::approved()),
            Inner::Test { canned } => Ok(canned
                .iter()
                .find(|(k, _)| k == s3_key)
                .map(|(_, r)| r.clone())
                .unwrap_or_else(ModerationResult::approved)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    #[error("moderation service error: {0}")]
    Service(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

// ─────────────────────────────────────────────────────────────────────────────
// Handler: moderate one artwork image
// ─────────────────────────────────────────────────────────────────────────────

/// Load the image row, ask the client for a verdict, write the result
/// back to `artwork_images.moderation_status`. Idempotent: running
/// twice on the same row replays the same verdict (and writes the
/// same value) — fine for at-least-once delivery.
///
/// Returns `Ok(())` on missing row (already deleted / never existed) —
/// no point retrying.
pub async fn moderate_artwork_image(
    client: &ModerationClient,
    pool: &crate::db::Pool,
    artwork_image_id: uuid::Uuid,
) -> Result<(), ModerationError> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"SELECT s3_key FROM artwork_images WHERE id = $1"#,
    )
    .bind(artwork_image_id)
    .fetch_optional(pool)
    .await?;

    let Some((s3_key,)) = row else {
        tracing::debug!(%artwork_image_id, "artwork_image row gone before moderation ran");
        return Ok(());
    };

    let result = client.moderate(&s3_key).await?;

    if matches!(result.status, ModerationStatus::Rejected) {
        tracing::warn!(
            %artwork_image_id,
            labels = ?result.labels,
            "artwork_image rejected by moderation",
        );
    } else {
        tracing::debug!(%artwork_image_id, "artwork_image approved");
    }

    sqlx::query(
        r#"UPDATE artwork_images
           SET moderation_status = $2
           WHERE id = $1"#,
    )
    .bind(artwork_image_id)
    .bind(result.status.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

/// T-008b — moderate one visual-search upload row.
///
/// Mirrors `moderate_artwork_image` but targets the `uploads` table
/// (visual-search anchor images). On rejection, the search path's
/// `moderation_status != 'rejected'` filter starts hiding the row;
/// the public S3 object is left in place (the cleanup job evicts it
/// via `expires_at`).
///
/// `Ok(())` on missing row — same noop semantics as the artwork
/// variant.
pub async fn moderate_upload(
    client: &ModerationClient,
    pool: &crate::db::Pool,
    upload_id: uuid::Uuid,
) -> Result<(), ModerationError> {
    let row: Option<(String,)> =
        sqlx::query_as(r#"SELECT s3_key FROM uploads WHERE id = $1"#)
            .bind(upload_id)
            .fetch_optional(pool)
            .await?;

    let Some((s3_key,)) = row else {
        tracing::debug!(%upload_id, "upload row gone before moderation ran");
        return Ok(());
    };

    let result = client.moderate(&s3_key).await?;

    if matches!(result.status, ModerationStatus::Rejected) {
        tracing::warn!(
            %upload_id,
            labels = ?result.labels,
            "upload rejected by moderation",
        );
    } else {
        tracing::debug!(%upload_id, "upload approved");
    }

    sqlx::query(r#"UPDATE uploads SET moderation_status = $2 WHERE id = $1"#)
        .bind(upload_id)
        .bind(result.status.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_client_approves() {
        let c = ModerationClient::disabled();
        assert!(!c.enabled());
        let r = c.moderate("uploads/anything.jpg").await.unwrap();
        assert_eq!(r.status, ModerationStatus::Approved);
        assert!(r.labels.is_empty());
    }

    #[tokio::test]
    async fn test_client_returns_canned_result() {
        let c = ModerationClient::for_tests(vec![
            (
                "uploads/bad.jpg".to_string(),
                ModerationResult::rejected(vec!["Explicit Nudity".to_string()]),
            ),
            ("uploads/good.jpg".to_string(), ModerationResult::approved()),
        ]);
        let bad = c.moderate("uploads/bad.jpg").await.unwrap();
        assert_eq!(bad.status, ModerationStatus::Rejected);
        assert_eq!(bad.labels, vec!["Explicit Nudity".to_string()]);

        let good = c.moderate("uploads/good.jpg").await.unwrap();
        assert_eq!(good.status, ModerationStatus::Approved);

        // Unknown s3_key falls back to approved.
        let other = c.moderate("uploads/other.jpg").await.unwrap();
        assert_eq!(other.status, ModerationStatus::Approved);
    }

    #[tokio::test]
    async fn test_client_canned_works_for_uploads_keys_too() {
        // The same client is used for artwork_images + uploads.
        // Sanity-check that an `uploads/...` s3_key resolves to the
        // canned verdict — used in the T-008b integration tests.
        let c = ModerationClient::for_tests(vec![(
            "uploads/abc.jpg".to_string(),
            ModerationResult::rejected(vec!["Violence".to_string()]),
        )]);
        let r = c.moderate("uploads/abc.jpg").await.unwrap();
        assert_eq!(r.status, ModerationStatus::Rejected);
    }

    #[test]
    fn status_as_str_matches_db_check_constraint() {
        assert_eq!(ModerationStatus::Approved.as_str(), "approved");
        assert_eq!(ModerationStatus::Rejected.as_str(), "rejected");
    }
}
