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
pub mod series;
pub mod settings;

use ml_art_core::{auth::User, db::Pool, error::ApiError};
use uuid::Uuid;

/// Resolve the caller's `Artist` id. Returns `NotFound` for users that
/// have no artist row linked (i.e. signed-in collectors, not artists).
/// Use this as the very first call in every studio handler — it's the
/// gate that turns a generic `AuthedUser` into "the artist the request
/// is allowed to act on."
pub async fn current_artist_id(pool: &Pool, user: &User) -> Result<Uuid, ApiError> {
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
