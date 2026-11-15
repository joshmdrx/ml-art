//! `/v1/studio/*` — the authenticated artist's own data.
//!
//! Every endpoint here is gated on the caller being an artist (i.e. their
//! `users` row has `is_artist = true` AND an `artists` row points back
//! via `user_id`). Ownership is enforced in SQL: a handler that takes an
//! artwork id always joins through `artists.user_id = $current_user.id`
//! and returns 404 — not 403 — when no row matches, to avoid leaking the
//! existence of other artists' artworks.
//!
//! T-011 Phase 1: artwork CRUD. Settings, analytics, inquiries-received,
//! and bulk uploads live in later phases.

pub mod artworks;
pub mod inquiries;
pub mod locations;
pub mod me;
pub mod orders;
pub mod series;
pub mod settings;
pub mod stripe;

use ml_art_core::{auth::User, db::Pool, error::ApiError};
use uuid::Uuid;

/// Resolve the caller's `Artist` id. Returns `NotFound` for users that
/// have no artist row linked (i.e. signed-in collectors, not artists).
/// Use this as the very first call in every studio handler — it's the
/// gate that turns a generic `AuthedUser` into "the artist the request
/// is allowed to act on."
///
/// T-086 — admins editing a draft profile resolve to that draft instead
/// (see `users.editing_draft_artist_id`, migration 0033). Honoured only
/// for admins and only while the target is still an unclaimed draft, so
/// a claim or takedown ends the editing session without extra cleanup.
pub async fn current_artist_id(pool: &Pool, user: &User) -> Result<Uuid, ApiError> {
    if user.is_admin {
        if let Some(draft_id) = editing_draft_id(pool, user.id).await? {
            return Ok(draft_id);
        }
    }
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM artists
        WHERE user_id = $1
          AND deleted_at IS NULL
        "#,
    )
    .bind(user.id)
    .fetch_optional(pool)
    .await?;
    row.map(|(id,)| id).ok_or(ApiError::NotFound)
}

/// T-086 — the unclaimed draft this user is currently editing, if any.
/// Callers must check `is_admin` first; this function doesn't.
pub async fn editing_draft_id(pool: &Pool, user_id: Uuid) -> Result<Option<Uuid>, ApiError> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT a.id
        FROM users u
        JOIN artists a ON a.id = u.editing_draft_artist_id
        WHERE u.id = $1
          AND a.is_prebuilt
          AND a.user_id IS NULL
          AND a.deleted_at IS NULL
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

/// T-086 — true when `artist_id` is a draft nobody has claimed yet.
/// Used to refuse anything that would publish a draft without the
/// organisation's say-so (studio status toggle, admin approve).
pub async fn is_unclaimed_draft(pool: &Pool, artist_id: Uuid) -> Result<bool, ApiError> {
    let row: Option<(bool,)> = sqlx::query_as(
        r#"
        SELECT (is_prebuilt AND user_id IS NULL)
        FROM artists
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(artist_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some_and(|(b,)| b))
}

/// T-086 — the first time a claimed draft page goes `pending → active`,
/// publish the artworks the admin built as drafts. The organisation
/// approved the page as they saw it in the preview (which shows draft
/// artworks), so it should go live with those works rather than empty.
/// Later publishes don't touch artworks: by then a `draft` artwork is a
/// deliberate choice by the owner.
pub async fn publish_claimed_draft_artworks(
    pool: &Pool,
    artist_id: Uuid,
    previous_status: &str,
    new_status: &str,
) -> Result<u64, ApiError> {
    if previous_status != "pending" || new_status != "active" {
        return Ok(0);
    }
    let res = sqlx::query(
        r#"
        UPDATE artworks SET
            status = 'published',
            published_at = COALESCE(published_at, now()),
            updated_at = now()
        WHERE artist_id = $1
          AND status = 'draft'
          AND deleted_at IS NULL
          AND EXISTS (
              SELECT 1 FROM artists
              WHERE id = $1 AND is_prebuilt AND user_id IS NOT NULL
          )
        "#,
    )
    .bind(artist_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}
